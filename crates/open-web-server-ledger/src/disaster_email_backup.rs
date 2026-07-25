//! スタンドアロンのメール・ディザスタバックアップ(`disaster_email_backup`
//! feature)。
//!
//! **設計方針(ユーザー指示、2026-07-25)**: 他のVPSへの分散同期・
//! `MultiRegionReplicator`のようなより重い冗長化機能を一切設定しなくても、
//! **メールアドレスひとつだけ**で有効化できる、最小構成のディザスタ
//! セーフティネットを提供する。SATA/USB/LAN/WiFi等の物理的な断線・
//! ネットワーク障害により、`Ledger::commit`の権威パス(open-runo経由の
//! 3ホップコミット)や`MultiRegionReplicator`が失敗した場合に、
//! **失われかけているmutation自体をメールで退避する**、最後の砦。
//!
//! **再利用方針(車輪の再発明をしない)**: メール送信ロジック自体は
//! 姉妹リポジトリ`open-raid-z`が実装・テスト済みの
//! `open_raid_z_core::offsite_backup::EmailBackupTarget`をそのまま
//! path依存で再利用する。このモジュールが新規に持つのは、
//! (a) `open-web-server-ledger`固有の型(`MutationRequest`)をバックアップ
//! セグメントへ変換する薄い橋渡し、(b) 失敗を握りつぶさず記録する
//! ベストエフォートの送信ラッパー、の2点のみ。
//!
//! **正直な開示**: (1) 実SMTPサーバー・実メールアカウントへの接続は
//! このモジュールのテストでは一切行っていない(`open-raid-z`側の
//! `tests/offsite_backup_integration.rs`と同じ「ローカルモックSMTPのみ」
//! 方針)。(2) 実際の物理断線(SATA/USB/LAN/WiFiケーブル抜去)を検知する
//! 専用のハードウェアイベントフックはこのリポジトリには無い——
//! `Ledger::commit`が権威パス(open-runo経由)・マルチリージョン
//! レプリケーションのいずれかで実際に失敗した時点を「断線・障害相当」の
//! シグナルとして扱う(`open-raid-z`の`disaster_recovery.rs`が持つ
//! 「再接続時自動復旧」とは異なり、こちらは送信のみでリプレイ機構は
//! 持たない——スコープは「消えかけているデータをメールで見える化する」
//! ことに限定した安全側の設計)。

use anyhow::Context;
use open_raid_z_core::offsite_backup::{EmailBackupTarget, EmailBackupTargetConfig, OffsiteBackupTarget};
use open_web_server_core::MutationRequest;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

/// 管理API経由で受け取る設定。`EmailBackupTargetConfig`をそのまま
/// 包むだけ(このリポジトリ固有のフィールドは追加しない——「メール
/// アドレスひとつだけで有効化できる」という要件に沿い、必須項目は
/// `open_raid_z_core`側が既に定義済みの最小限のSMTP設定のみ)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisasterEmailBackupConfig {
    #[serde(flatten)]
    pub email: EmailBackupTargetConfig,
}

/// `Ledger`から独立して使えるスタンドアロンのメール退避ラッパー。
/// VPS同期先・マルチリージョンレプリケータの登録有無に一切依存しない。
pub struct DisasterEmailBackup {
    target: EmailBackupTarget,
}

impl DisasterEmailBackup {
    pub fn new(config: DisasterEmailBackupConfig) -> Self {
        Self { target: EmailBackupTarget::new(config.email) }
    }

    /// SMTP接続の疎通確認のみ(実送信は行わない)。
    pub fn ensure_ready(&self) -> anyhow::Result<()> {
        self.target
            .ensure_ready()
            .map_err(|e| anyhow::anyhow!("disaster email backup not ready: {e}"))
    }

    /// 権威パス(open-runo経由の3ホップコミット、またはマルチリージョン
    /// 同期レプリケーション)が実際に失敗した`MutationRequest`をメールで
    /// 退避する。**ベストエフォート**——このメソッド自体が失敗しても
    /// 呼び出し元(`Ledger::commit`)の失敗理由を上書きしない設計とする
    /// (呼び出し側で`tokio::task::spawn_blocking`等を通じ、結果をログに
    /// 残すに留める使い方を想定)。
    pub fn backup_failed_mutation(&self, req: &MutationRequest, reason: &str) -> anyhow::Result<()> {
        let label = format!("disaster-fallback-{}.json", req.idempotency_key.0);
        let payload = serde_json::json!({
            "reason": reason,
            "idempotency_key": req.idempotency_key.0,
            "account_id": req.account_id,
            "target": req.target,
            "payload": req.payload,
            "requested_at": req.requested_at,
        });
        let bytes = serde_json::to_vec_pretty(&payload).context("failed to serialize mutation for email backup")?;

        match self.target.upload_segment(&label, &bytes) {
            Ok(()) => {
                info!(key = %req.idempotency_key.0, reason, "disaster email backup: mutation emailed as fallback");
                Ok(())
            }
            Err(e) => {
                error!(key = %req.idempotency_key.0, reason, error = %e, "disaster email backup: failed to email fallback segment");
                Err(anyhow::anyhow!("disaster email backup failed: {e}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_web_server_core::IdempotencyKey;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    /// `open-raid-z`側`tests/offsite_backup_integration.rs`と同じ
    /// 最小限の偽SMTPサーバー(EHLO/AUTH LOGIN/MAIL FROM/RCPT TO/DATA/QUIT)。
    /// 実SMTPサーバーへは一切接続しない。
    fn spawn_fake_smtp_server(received: Arc<Mutex<Vec<String>>>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                handle_smtp_client(stream, Arc::clone(&received));
                break;
            }
        });
        port
    }

    fn handle_smtp_client(mut stream: TcpStream, received: Arc<Mutex<Vec<String>>>) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let _ = stream.write_all(b"220 localhost fake smtp ready\r\n");
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let cmd = line.trim_end();
            if cmd.to_ascii_uppercase().starts_with("EHLO") {
                let _ = stream.write_all(b"250-localhost\r\n250-AUTH LOGIN\r\n250 OK\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("AUTH LOGIN") {
                let _ = stream.write_all(b"334 VXNlcm5hbWU6\r\n");
                line.clear();
                reader.read_line(&mut line).unwrap();
                let _ = stream.write_all(b"334 UGFzc3dvcmQ6\r\n");
                line.clear();
                reader.read_line(&mut line).unwrap();
                let _ = stream.write_all(b"235 Authentication successful\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("MAIL FROM") {
                let _ = stream.write_all(b"250 OK\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("RCPT TO") {
                let _ = stream.write_all(b"250 OK\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("DATA") {
                let _ = stream.write_all(b"354 Start mail input; end with <CRLF>.<CRLF>\r\n");
                let mut body = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if line == ".\r\n" {
                        break;
                    }
                    body.push_str(&line);
                }
                received.lock().unwrap().push(body);
                let _ = stream.write_all(b"250 OK: queued\r\n");
            } else if cmd.to_ascii_uppercase().starts_with("QUIT") {
                let _ = stream.write_all(b"221 Bye\r\n");
                return;
            } else {
                let _ = stream.write_all(b"250 OK\r\n");
            }
        }
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

    #[test]
    fn backup_failed_mutation_sends_json_payload_via_mock_smtp() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let port = spawn_fake_smtp_server(Arc::clone(&received));

        let backup = DisasterEmailBackup::new(DisasterEmailBackupConfig {
            email: EmailBackupTargetConfig {
                smtp_host: "127.0.0.1".to_string(),
                smtp_port: port,
                smtp_username: "backup@example.com".to_string(),
                smtp_password_env: "OWS_TEST_SMTP_PASSWORD_DOES_NOT_EXIST".to_string(),
                from_address: "backup@example.com".to_string(),
                to_address: "admin@example.com".to_string(),
                allow_plaintext_for_testing: true,
            },
        });

        // lettreはSMTP認証にパスワードを要求するため、テスト内でのみ
        // 環境変数を設定する(実運用の秘密情報とは無関係なテスト専用値)。
        std::env::set_var("OWS_TEST_SMTP_PASSWORD_DOES_NOT_EXIST", "test-password");

        let req = sample_request("disaster-key-1");
        backup
            .backup_failed_mutation(&req, "upstream commit failed after retries")
            .expect("email backup should succeed against the mock smtp server");

        let bodies = received.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].contains("disaster-key-1"));
    }

    /// VPS同期先・マルチリージョンレプリケータを一切構築せず、
    /// `DisasterEmailBackup`単体だけを構築・使用できることを確認する
    /// (要件どおり「メールアドレスひとつだけ」で完結すること)。
    #[test]
    fn disaster_email_backup_requires_no_other_registry_or_replicator() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let port = spawn_fake_smtp_server(Arc::clone(&received));
        std::env::set_var("OWS_TEST_SMTP_PASSWORD_STANDALONE", "test-password");

        // Ledger/MultiRegionReplicator/VPS同期先レジストリのいずれも
        // 生成していない——これがコンパイル・実行できること自体が
        // 「独立して動く」ことの検証になる。
        let backup = DisasterEmailBackup::new(DisasterEmailBackupConfig {
            email: EmailBackupTargetConfig {
                smtp_host: "127.0.0.1".to_string(),
                smtp_port: port,
                smtp_username: "backup@example.com".to_string(),
                smtp_password_env: "OWS_TEST_SMTP_PASSWORD_STANDALONE".to_string(),
                from_address: "backup@example.com".to_string(),
                to_address: "admin@example.com".to_string(),
                allow_plaintext_for_testing: true,
            },
        });

        backup
            .backup_failed_mutation(&sample_request("standalone-key-1"), "simulated disconnection")
            .expect("standalone email backup should work with no VPS/multi-region setup");
    }
}
