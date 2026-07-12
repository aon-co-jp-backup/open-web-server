//! open-web-server-ledger
//!
//! 3Dオンラインゲームの課金アイテムや、金融/クレジットカード情報が
//! ネット上で「消失しない」ことを保証するための書き込みパイプライン。
//!
//! ## 3層コミット (open-web-server → open-runo → aruaru-db)
//!
//! ```text
//! Client
//!   │  (3層防御通信: open-web-server-wire)
//!   ▼
//! open-web-server  ── ① ローカル WAL に先行書き込み (fsync)
//!   │  (3層防御通信)
//!   ▼
//! open-runo         ── ② Gateway が Federation 経由でルーティング・監査ログ記録
//!   │  (3層防御通信)
//!   ▼
//! aruaru-db          ── ③ Raft 分散合意でコミット (Git-on-SQL コミットIDを発行)
//! ```
//!
//! 各段階は `IdempotencyKey` を伝播させるため、途中で再送されても
//! 二重課金・二重付与が起きない。③ の commit_id が返るまでクライアントには
//! 「確定」を返さない (at-least-once + 冪等 = 実質 exactly-once)。
//!
//! ## UDP-IP 冗長経路 (2026-07-11 追加、副系・ベストエフォート)
//!
//! `open-web-server-wire::udp_channel` を使い、①の直後に**同じ
//! `MutationRequest` を、TCP経由の②(open-runoへのforward)と並行して
//! UDPでも即時送出**する (`Ledger::enable_udp_redundant_path` で有効化した
//! 場合のみ)。UDP送出は `tokio::spawn` した別タスクの fire-and-forget で
//! あり、失敗・タイムアウトしても TCP経由の権威パスには一切影響しない
//! (このモジュールの統合テストで実証)。UDP側は「即時通知/advance notice」
//! に過ぎず、正式なコミット確定 (`db_commit_id` の発行) は今まで通り
//! TCP経由の3ホップコミットのみが担う。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use open_web_server_core::{CoreError, CoreResult, MutationReceipt, MutationRequest};
use open_web_server_wire::udp_channel::{UdpChannelKeys, UdpSender};
use tracing::{info, warn};

pub mod audit_log;
pub mod postgres_wal;
pub use audit_log::{AuditRecord, FileAuditLog, ReconciliationReport};
pub use postgres_wal::PostgresWal;

#[derive(Clone)]
pub struct LedgerConfig {
    /// open-runo Gateway のエンドポイント (例: https://runo.internal:8443)
    pub open_runo_endpoint: String,
    pub max_retries: u32,
    pub retry_backoff: Duration,
}

/// WAL (Write-Ahead Log) の最小インターフェース。
/// 実装は sled / RocksDB / aruaru-db 自身などに差し替え可能にしておく。
#[async_trait::async_trait]
pub trait WriteAheadLog: Send + Sync {
    async fn append(&self, req: &MutationRequest) -> anyhow::Result<()>;
    async fn mark_committed(&self, key: &str, commit_id: &str) -> anyhow::Result<()>;
    async fn is_already_processed(&self, key: &str) -> anyhow::Result<Option<MutationReceipt>>;
}

pub struct Ledger {
    config: LedgerConfig,
    wal: Arc<dyn WriteAheadLog>,
    http: reqwest::Client,
    /// UDP-IP 冗長経路 (副系、ベストエフォート)。未設定ならTCP経路のみで動作する。
    udp_redundant_path: Option<UdpRedundantPath>,
    /// 独立監査/突き合わせログ (拡張要件(4)-④、DATABASE書き込み四重化の
    /// 4本目)。WAL/aruaru-db/PostgreSQLのいずれとも技術的に独立した
    /// 追記専用ファイル。未設定なら従来通り記録されない (任意機能)。
    audit_log: Option<Arc<audit_log::FileAuditLog>>,
}

struct UdpRedundantPath {
    sender: Arc<UdpSender>,
    dest: SocketAddr,
}

impl Ledger {
    pub fn new(config: LedgerConfig, wal: Arc<dyn WriteAheadLog>) -> Self {
        Self {
            config,
            wal,
            http: reqwest::Client::new(),
            udp_redundant_path: None,
            audit_log: None,
        }
    }

    /// 独立監査ログ (拡張要件(4)-④) を有効化する。①PostgreSQL・②aruaru-db・
    /// ③マルチリージョン同期レプリケーションのいずれとも技術的に独立した
    /// 4本目の永続化先として、コミット試行ごとに1レコードを追記する。
    /// 呼び出しは任意であり、有効化しなくても従来通り動作する。
    pub fn enable_audit_log(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.audit_log = Some(Arc::new(audit_log::FileAuditLog::new(path)));
        self
    }

    /// 有効化済みの独立監査ログへの参照 (突き合わせ/検証をアプリ側から
    /// 呼び出せるようにするため公開する)。
    pub fn audit_log(&self) -> Option<Arc<audit_log::FileAuditLog>> {
        self.audit_log.clone()
    }

    /// UDP-IP 冗長経路を有効化する。`bind_addr` はこのプロセスがUDP送信に
    /// 使うローカルソケット (通常 `0.0.0.0:0` 等の任意ポート)、`dest` は
    /// 副系の受信先 (open-runo側のUDPリスナー、今回のスコープ外の別実装)。
    /// 呼び出しは任意であり、有効化しなくてもTCP経路のみで従来通り動作する。
    pub async fn enable_udp_redundant_path(
        mut self,
        bind_addr: SocketAddr,
        dest: SocketAddr,
        keys: &UdpChannelKeys,
    ) -> anyhow::Result<Self> {
        let sender = UdpSender::bind(bind_addr, keys).await?;
        self.udp_redundant_path = Some(UdpRedundantPath {
            sender: Arc::new(sender),
            dest,
        });
        Ok(self)
    }

    /// 課金/金融データの書き込みを、消失しない形で確定させる。
    pub async fn commit(&self, req: MutationRequest) -> CoreResult<MutationReceipt> {
        // 冪等性チェック: 同じキーが既に処理済みならそのまま返す (二重書き込み拒否)
        if let Some(existing) = self
            .wal
            .is_already_processed(&req.idempotency_key.0)
            .await
            .map_err(|e| CoreError::Validation(e.to_string()))?
        {
            warn!(key = %req.idempotency_key.0, "duplicate mutation request detected");
            return Ok(existing);
        }

        // ① ローカル WAL に先行書き込み (fsync 相当)。ここで確定すればプロセスが
        //    落ちても再起動時にリプレイでき、ネットワーク到達前のデータ消失を防ぐ。
        self.wal
            .append(&req)
            .await
            .map_err(|e| CoreError::Validation(e.to_string()))?;

        // UDP-IP 冗長経路: ベストエフォートの即時通知。fire-and-forgetで
        // spawnし、TCP経由の権威パス (下のforward_with_retry) を
        // ブロックしない・失敗させない。
        self.fire_udp_redundant_notice(&req);

        // 独立監査ログ (拡張要件(4)-④): WAL/aruaru-db/PostgreSQLとは技術的に
        // 独立した4本目の永続化先へ、コミット試行の時点で1レコード追記する。
        // 書き込み失敗は監査ログ自体の不具合であり、権威パス (aruaru-dbへの
        // 実コミット) をブロック・失敗させてはならないため警告ログのみに留める
        // (UDP冗長経路と同じ「補助系はブロックしない」設計方針)。
        if let Some(audit) = &self.audit_log {
            let record = audit_log::AuditRecord::from_request(&req);
            if let Err(e) = audit.append(&record) {
                warn!(key = %req.idempotency_key.0, error = %e, "audit log append failed (independent of authoritative path)");
            }
        }

        // ② open-runo 経由で aruaru-db にコミット要求を送る (3層防御通信を利用)
        let commit_id = self.forward_with_retry(&req).await?;

        self.wal
            .mark_committed(&req.idempotency_key.0, &commit_id)
            .await
            .map_err(|e| CoreError::Validation(e.to_string()))?;

        info!(key = %req.idempotency_key.0, commit_id, "mutation committed");

        Ok(MutationReceipt {
            idempotency_key: req.idempotency_key,
            committed: true,
            db_commit_id: Some(commit_id),
            committed_at: Some(chrono::Utc::now()),
        })
    }

    /// UDP冗長経路が有効なら、同じミューテーションを別タスクで即時送出する。
    /// 送信失敗・宛先未リッスンは警告ログのみで、`commit` の結果には一切影響しない。
    fn fire_udp_redundant_notice(&self, req: &MutationRequest) {
        let Some(path) = &self.udp_redundant_path else {
            return;
        };
        let sender = Arc::clone(&path.sender);
        let dest = path.dest;
        let req = req.clone();
        let key = req.idempotency_key.0.clone();
        tokio::spawn(async move {
            if let Err(e) = sender.send_mutation(dest, &req).await {
                warn!(key = %key, error = %e, "UDP redundant path send failed (best-effort, TCP path unaffected)");
            }
        });
    }

    async fn forward_with_retry(&self, req: &MutationRequest) -> CoreResult<String> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.forward_once(req).await {
                Ok(commit_id) => return Ok(commit_id),
                Err(e) if attempt < self.config.max_retries => {
                    warn!(attempt, error = %e, "forward to open-runo failed, retrying");
                    tokio::time::sleep(self.config.retry_backoff * attempt).await;
                }
                Err(e) => {
                    return Err(CoreError::UpstreamCommitFailed(e.to_string()));
                }
            }
        }
    }

    async fn forward_once(&self, req: &MutationRequest) -> anyhow::Result<String> {
        let url = format!("{}/internal/db/mutate", self.config.open_runo_endpoint);
        let resp = self
            .http
            .post(url)
            .json(req)
            .send()
            .await
            .context("open-runo request failed")?
            .error_for_status()
            .context("open-runo returned error status")?;

        let receipt: MutationReceipt = resp.json().await.context("invalid receipt body")?;
        receipt
            .db_commit_id
            .ok_or_else(|| anyhow::anyhow!("open-runo did not return a db_commit_id"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_web_server_core::IdempotencyKey;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockWal {
        processed: Mutex<Option<MutationReceipt>>,
        appended: Mutex<u32>,
    }

    #[async_trait::async_trait]
    impl WriteAheadLog for MockWal {
        async fn append(&self, _req: &MutationRequest) -> anyhow::Result<()> {
            *self.appended.lock().unwrap() += 1;
            Ok(())
        }

        async fn mark_committed(&self, _key: &str, _commit_id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn is_already_processed(
            &self,
            _key: &str,
        ) -> anyhow::Result<Option<MutationReceipt>> {
            Ok(self.processed.lock().unwrap().clone())
        }
    }

    fn test_config() -> LedgerConfig {
        LedgerConfig {
            open_runo_endpoint: "http://127.0.0.1:0".to_string(),
            max_retries: 1,
            retry_backoff: Duration::from_millis(1),
        }
    }

    #[tokio::test]
    async fn duplicate_idempotency_key_short_circuits_without_reappending() {
        let key = IdempotencyKey("11111111-1111-1111-1111-111111111111".to_string());
        let existing = MutationReceipt {
            idempotency_key: key.clone(),
            committed: true,
            db_commit_id: Some("commit-1".to_string()),
            committed_at: Some(chrono::Utc::now()),
        };
        let wal = Arc::new(MockWal {
            processed: Mutex::new(Some(existing.clone())),
            appended: Mutex::new(0),
        });
        let ledger = Ledger::new(test_config(), wal.clone());

        let req = MutationRequest {
            idempotency_key: key,
            account_id: "user-1".to_string(),
            target: "items".to_string(),
            payload: serde_json::json!({"item_id": "sword", "quantity": 1}),
            requested_at: chrono::Utc::now(),
        };

        let receipt = ledger.commit(req).await.expect("commit should succeed");

        assert_eq!(receipt.db_commit_id, existing.db_commit_id);
        assert_eq!(*wal.appended.lock().unwrap(), 0, "must not re-append a duplicate mutation");
    }

    fn sample_request(key: &str) -> MutationRequest {
        MutationRequest {
            idempotency_key: IdempotencyKey(key.to_string()),
            account_id: "user-1".to_string(),
            target: "items".to_string(),
            payload: serde_json::json!({"item_id": "sword", "quantity": 1}),
            requested_at: chrono::Utc::now(),
        }
    }

    /// 実TCPソケットで待ち受け、どんなリクエストが来ても固定のJSON
    /// `MutationReceipt` を返すごく単純なモックHTTPサーバ。open-runoの
    /// 代わりに使い、`Ledger::commit` のTCP経由コミットが実際に成立する
    /// ことを検証する。
    async fn spawn_mock_open_runo(commit_id: &str) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let commit_id = commit_id.to_string();

        let handle = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let commit_id = commit_id.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    // リクエスト全体は読み切らず、送られてくる分だけ読み捨てる。
                    let _ = stream.read(&mut buf).await;

                    let body = serde_json::json!({
                        "idempotency_key": "placeholder",
                        "committed": true,
                        "db_commit_id": commit_id,
                        "committed_at": chrono::Utc::now(),
                    })
                    .to_string();
                    // idempotency_key はテスト側で使わないため固定値でよい
                    // (受信側は open-runo からの受領票の db_commit_id のみを見る)。
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });

        (format!("http://{addr}"), handle)
    }

    /// UDP冗長経路が有効かつ到達可能な場合、TCP経由の権威パスと並行して
    /// 送出されたUDP通知が受信側で正しく復号・デデュープされることを、
    /// 実UDPソケット (loopback) で検証する。
    #[tokio::test]
    async fn udp_redundant_path_delivers_and_dedups_against_simulated_tcp_delivery() {
        use open_web_server_wire::udp_channel::{UdpChannelKeys, UdpReceiver};

        let (endpoint, _server) = spawn_mock_open_runo("commit-udp-1").await;
        let wal = Arc::new(MockWal::default());
        let keys = UdpChannelKeys::generate_for_testing();

        let receiver = UdpReceiver::bind("127.0.0.1:0".parse().unwrap(), &keys)
            .await
            .unwrap();
        let recv_addr = receiver.local_addr().unwrap();

        let ledger = Ledger::new(
            LedgerConfig {
                open_runo_endpoint: endpoint,
                max_retries: 1,
                retry_backoff: Duration::from_millis(1),
            },
            wal,
        )
        .enable_udp_redundant_path("127.0.0.1:0".parse().unwrap(), recv_addr, &keys)
        .await
        .unwrap();

        let req = sample_request("udp-dedup-key-1");

        // TCP経由の権威パスを確定させる (これがidempotency keyの「本採用」)。
        let receipt = ledger.commit(req.clone()).await.expect("tcp commit must succeed");
        assert_eq!(receipt.db_commit_id.as_deref(), Some("commit-udp-1"));

        // 並行して spawn されたUDP送出が受信側に届くのを待つ。
        let first = tokio::time::timeout(Duration::from_secs(2), receiver.recv_mutation())
            .await
            .expect("udp notice should arrive within timeout")
            .expect("udp recv must not error");
        assert_eq!(
            first.map(|r| r.idempotency_key.0),
            Some("udp-dedup-key-1".to_string()),
            "udp side must deliver the same idempotency key as the tcp-committed mutation"
        );

        // 同一キーの mutation が UDP 経由でもう一度届いた状況をシミュレートする
        // (TCPとUDPの二重到達は起こり得るが、デデュープにより実害がないこと)。
        if let Some(path) = &ledger.udp_redundant_path {
            path.sender.send_mutation(recv_addr, &req).await.unwrap();
        }
        let second = tokio::time::timeout(Duration::from_secs(2), receiver.recv_mutation())
            .await
            .expect("second udp datagram should arrive")
            .expect("udp recv must not error");
        assert!(
            second.is_none(),
            "duplicate idempotency key over udp must be deduplicated"
        );
    }

    /// UDP冗長経路の宛先が誰もlistenしていない (閉じたポート相当) 場合でも、
    /// TCP経由の権威パスは影響を受けず、mutationは正常にコミットされることを
    /// 実証する。これが「UDP障害はTCP経路をブロック・破壊しない」という
    /// 設計上の保証の直接的な検証にあたる。
    #[tokio::test]
    async fn tcp_authoritative_path_succeeds_even_when_udp_path_is_entirely_unreachable() {
        use open_web_server_wire::udp_channel::UdpChannelKeys;

        let (endpoint, _server) = spawn_mock_open_runo("commit-udp-2").await;
        let wal = Arc::new(MockWal::default());
        let keys = UdpChannelKeys::generate_for_testing();

        // 誰もbindしていない宛先アドレス (UDPは受信側不在でも送信自体は
        // OSレベルで成功しうるため、意図的に到達不能な設定を模す)。
        let unreachable_udp_dest: SocketAddr = "127.0.0.1:1".parse().unwrap();

        let ledger = Ledger::new(
            LedgerConfig {
                open_runo_endpoint: endpoint,
                max_retries: 1,
                retry_backoff: Duration::from_millis(1),
            },
            wal,
        )
        .enable_udp_redundant_path("127.0.0.1:0".parse().unwrap(), unreachable_udp_dest, &keys)
        .await
        .unwrap();

        let req = sample_request("udp-unreachable-key-1");

        let receipt = tokio::time::timeout(Duration::from_secs(5), ledger.commit(req))
            .await
            .expect("commit must not hang even if the udp path is unreachable")
            .expect("tcp-authoritative commit must still succeed");

        assert!(receipt.committed);
        assert_eq!(receipt.db_commit_id.as_deref(), Some("commit-udp-2"));
    }

    /// `Ledger::commit` を経由すると、独立監査ログ (拡張要件(4)-④) に
    /// 実際にレコードが追記され、かつファイル内容がチェックサム検証を
    /// 通ることを実証する (WAL/TCP権威パスとは別の、実ファイルI/Oを伴う
    /// 検証)。
    #[tokio::test]
    async fn commit_appends_a_verifiable_record_to_the_independent_audit_log() {
        let (endpoint, _server) = spawn_mock_open_runo("commit-audit-1").await;
        let wal = Arc::new(MockWal::default());

        let mut audit_path = std::env::temp_dir();
        audit_path.push(format!(
            "open-web-server-ledger-audit-integration-{}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&audit_path);

        let ledger = Ledger::new(
            LedgerConfig {
                open_runo_endpoint: endpoint,
                max_retries: 1,
                retry_backoff: Duration::from_millis(1),
            },
            wal,
        )
        .enable_audit_log(&audit_path);

        let req = sample_request("audit-key-1");
        let receipt = ledger.commit(req).await.expect("commit should succeed");
        assert_eq!(receipt.db_commit_id.as_deref(), Some("commit-audit-1"));

        let audit = ledger.audit_log().expect("audit log should be enabled");
        let records = audit
            .scan_and_verify()
            .expect("audit log must pass checksum verification");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].idempotency_key, "audit-key-1");

        let report = audit
            .reconcile(&[IdempotencyKey("audit-key-1".to_string())])
            .expect("reconcile should succeed");
        assert!(report.missing_from_wal.is_empty());
        assert!(report.duplicate_in_audit_log.is_empty());
        assert_eq!(report.total_audit_records, 1);

        let _ = std::fs::remove_file(&audit_path);
    }
}
