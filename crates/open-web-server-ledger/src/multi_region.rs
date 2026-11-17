//! マルチリージョン同期レプリケーション (拡張要件(4) 「DATABASE書き込みの
//! 四重化」の**③マルチリージョン同期レプリケーション**の第一実装)
//!
//! ## `open-runo-db::FederatedBackend` を土台にしなかった理由 (2026-07-13、調査結果)
//!
//! `open-runo`/`poem-cosmo-tauri` 側の `open-runo-db::federated::FederatedBackend`
//! (`FederatedBuilder`: 複数の named member + テーブル単位ルーティング/
//! broadcast) を先に読んだ。汎用的な「複数DBを1つに統合運用する」ための
//! 抽象としてはよく出来ているが、以下の理由でこのリポジトリの③には
//! そのまま被せず、`open-web-server-ledger` 内に自己完結した実装を
//! 新設する判断とした:
//!
//! 1. **依存の重さ**: `open-web-server` は現状 `open-runo-db` に一切
//!    依存していない (このリポジトリ固有のCLAUDE.md「既知ギャップ」に
//!    記載の通り、`open-web-server-gateway` はまだPoem依存除去すら
//!    済んでいない発展途上のリポジトリ)。`open-runo-db`は`open-runo`/
//!    `poem-cosmo-tauri`側の18クレート構成workspaceに属するクレートで、
//!    そのpathを跨いでこちらから依存させると、2リポジトリの独立した
//!    バージョニング・リリースサイクルを密結合させてしまう。
//! 2. **意味論のズレ**: `FederatedBackend`はテーブル単位のルーティング/
//!    broadcast(「このテーブルはこのメンバーが所有する」)が主眼で、
//!    「同一のミューテーションを複数リージョンへ同期的に複製し、
//!    全リージョンのACKを待ってから呼び出し元に成功を返す」という
//!    ①②④と同列の「DB書き込みレグ」の意味論(このモジュールが実装する
//!    もの)とは設計目的が異なる。`broadcast()`は「fire to all, don't
//!    wait for a specific ack threshold policy」であり、失敗時の
//!    ポリシー(全滅で失敗 or N-of-M縮退)という要件を持たない。
//! 3. **このリポジトリの既存パターンとの一貫性**: `postgres_wal.rs`
//!    (①)・`audit_log.rs`(④)はいずれも`open-web-server-ledger`
//!    自己完結の実装であり、外部クレートは`sqlx`/`sha2`等の汎用ライブラリ
//!    のみに限定してきた。③も同じ流儀(自己完結・汎用クレートのみ)で
//!    揃えるのが一貫性がある。
//!
//! 上記の判断により、本モジュールは`sqlx`の`sqlite`feature (①PostgreSQL
//! WAL実装で既に依存済みの`sqlx`に、featureを1つ追加しただけ — 新規外部
//! クレートの追加は無し) を使い、**2つ以上の独立したSQLiteインスタンス**
//! を「リージョン」の代替として同期書き込みする。
//!
//! ## 同期性・失敗ポリシー (実際の設計判断)
//!
//! 「同期」とは、`replicate()` が **全リージョンへの書き込み結果が揃うまで
//! 呼び出し元に制御を返さない** ことを意味する — UDP冗長経路 (fire-and-
//! forget、`tokio::spawn`して即座に呼び出し元へ返る) とは対照的。
//!
//! 失敗時のポリシーは `min_acks` という閾値で表現する:
//!
//! - **デフォルト (`MultiRegionReplicator::new`) は `min_acks = regions.len()`
//!   (全リージョン必須)**。理由: `open-web-server/CLAUDE.md`
//!   拡張要件(4)が引用する [Best Database for Financial Data: Guide 2026]
//!   の通り、金融データにeventual consistencyは許されない — 1リージョンでも
//!   書き込みに失敗した状態を「成功」として呼び出し元(→ひいてはクライアント)
//!   に伝えることは、後にそのリージョンだけがデータを失った状態で
//!   読み出される (リージョン間の不整合) リスクを招く。よって規定値は
//!   最も厳格な「全滅で失敗」とする。
//! - **明示的に `with_min_acks(n)` を指定した場合のみ N-of-M へ縮退できる**
//!   (opt-in)。[Architecture Strategies for Designing for Redundancy,
//!   Microsoft Azure Well-Architected Framework] が指摘する通り、同期方式は
//!   一貫性を保つがレイテンシとのトレードオフがある — 可用性を優先したい
//!   運用者が意図的に選べるよう、閾値だけは呼び出し側に委ねる。ただし
//!   規定値を緩めることは無い(呼び出し側が明示的に選択しない限り、
//!   常に最も安全な「全リージョン必須」のまま)。
//!
//! `min_acks` 未満だった場合、成功したリージョンへの書き込みは**そのまま
//! 残す**(ロールバックしない — 各リージョンは独立したSQLiteファイルであり、
//! 分散トランザクションのコミット/ロールバックプロトコルは本実装のスコープ
//! 外。同一のidempotency_keyで再送されれば`INSERT ... ON CONFLICT DO
//! NOTHING`により残りのリージョンへの書き込みが冪等に完了する設計)。
//! 失敗の詳細(どのリージョンが失敗したか)は`RegionReplicationError`に
//! 全件保持して返す。

use open_web_server_core::MutationRequest;
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// 1つの「リージョン」(実体は独立した追記専用のJSON Linesファイル)。
///
/// 2026-11-18: 旧実装は`sqlx`のSQLite(Cの`libsqlite3-sys`に依存、
/// RUSTSEC-2026-0459)だったが、sqlxが修正版を許さない範囲に固定して
/// いるため、純Rustの追記専用ファイルへ置き換えた(C依存なし)。
/// 1行=1ミューテーション(`idempotency_key`が一意、同じキーの再送は
/// 追記せず無視=冪等)。本番のリージョンを`aruaru-db`(pgwire)にする
/// 実装はこの構造体の次の段階で、まだ無い。
#[derive(Clone)]
pub struct Region {
    pub name: String,
    path: PathBuf,
    /// 書き込み済みの`idempotency_key`(起動時にファイルから復元)。
    seen: Arc<Mutex<HashSet<String>>>,
}

impl Region {
    /// `path`(例: `region-tokyo.jsonl`)を開く(無ければ作る)。既存の行から
    /// 書き込み済みキーを復元する。
    pub async fn connect(name: impl Into<String>, path: &str) -> anyhow::Result<Self> {
        let path = PathBuf::from(path);
        let mut seen = HashSet::new();
        if path.exists() {
            for line in std::fs::read_to_string(&path)?.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let v: serde_json::Value = serde_json::from_str(line)?;
                if let Some(k) = v.get("idempotency_key").and_then(|k| k.as_str()) {
                    seen.insert(k.to_string());
                }
            }
        } else {
            std::fs::File::create(&path)?;
        }
        Ok(Self { name: name.into(), path, seen: Arc::new(Mutex::new(seen)) })
    }

    async fn write(&self, req: &MutationRequest) -> anyhow::Result<()> {
        let mut seen = self.seen.lock().map_err(|_| anyhow::anyhow!("region state poisoned"))?;
        if seen.contains(&req.idempotency_key.0) {
            return Ok(());
        }
        let line = serde_json::json!({
            "idempotency_key": req.idempotency_key.0,
            "account_id": req.account_id,
            "target": req.target,
            "payload": req.payload.to_string(),
            "requested_at": req.requested_at.to_rfc3339(),
        });
        // 既存ファイルにだけ追記する(ファイルが消えていれば本物のI/Oエラーになる)。
        let mut f = std::fs::OpenOptions::new().append(true).open(&self.path)?;
        writeln!(f, "{line}")?;
        f.sync_all()?;
        seen.insert(req.idempotency_key.0.clone());
        Ok(())
    }

    /// テスト専用: リージョンのファイルを消して同じ場所にディレクトリを置き、
    /// 以降の書き込みが本物のI/Oエラーで失敗するようにする(障害注入)。
    #[doc(hidden)]
    pub fn break_for_test(&self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::create_dir_all(&self.path);
    }

    /// 検証用: このリージョンに実際に着地した値(payload)を読み出す。
    pub async fn get(&self, idempotency_key: &str) -> anyhow::Result<Option<String>> {
        let text = std::fs::read_to_string(&self.path)?;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line)?;
            if v.get("idempotency_key").and_then(|k| k.as_str()) == Some(idempotency_key) {
                return Ok(v.get("payload").and_then(|p| p.as_str()).map(String::from));
            }
        }
        Ok(None)
    }
}

/// 1リージョンへの書き込み結果(失敗時の内訳をそのまま呼び出し元へ返すため)。
#[derive(Debug, Clone)]
pub struct RegionOutcome {
    pub region: String,
    pub error: Option<String>,
}

/// `min_acks`未満しか成功しなかった場合に返すエラー。
#[derive(Debug, thiserror::Error)]
#[error(
    "multi-region replication below threshold: {succeeded}/{total} succeeded, \
     min_acks={min_acks} required (failures: {failures:?})"
)]
pub struct MultiRegionError {
    pub succeeded: usize,
    pub total: usize,
    pub min_acks: usize,
    pub failures: Vec<RegionOutcome>,
}

/// 2つ以上の独立したリージョン(このサンドボックスでは実SQLiteファイル
/// 2つ以上)へ、同一のミューテーションを同期的に書き込む。
pub struct MultiRegionReplicator {
    regions: Vec<Region>,
    min_acks: usize,
}

impl MultiRegionReplicator {
    /// `regions`は2つ以上を推奨(1つだけでは「マルチ」リージョンにならない
    /// が、テスト容易性のため強制はしない)。デフォルトの`min_acks`は
    /// `regions.len()`(全リージョン必須、上記モジュールdoc参照)。
    pub fn new(regions: Vec<Region>) -> Self {
        let min_acks = regions.len();
        Self { regions, min_acks }
    }

    /// 明示的にN-of-M縮退を選択する場合のみ使う(opt-in、規定値は変えない)。
    /// `n`は`0..=regions.len()`にクランプする。
    #[must_use]
    pub fn with_min_acks(mut self, n: usize) -> Self {
        self.min_acks = n.min(self.regions.len());
        self
    }

    pub fn region_names(&self) -> Vec<&str> {
        self.regions.iter().map(|r| r.name.as_str()).collect()
    }

    pub fn region(&self, name: &str) -> Option<&Region> {
        self.regions.iter().find(|r| r.name == name)
    }

    /// 全リージョンへ並行して同期書き込みし、`min_acks`以上が成功して
    /// 初めて`Ok`を返す(呼び出し元は全リージョンの結果が揃うまでブロック
    /// される — これがUDP冗長経路との「同期/非同期」の違い)。
    pub async fn replicate(&self, req: &MutationRequest) -> Result<(), MultiRegionError> {
        let writes = self.regions.iter().map(|region| {
            let region = region.clone();
            let req = req.clone();
            async move {
                let result = region.write(&req).await;
                RegionOutcome {
                    region: region.name.clone(),
                    error: result.err().map(|e| e.to_string()),
                }
            }
        });

        let outcomes: Vec<RegionOutcome> = futures_join_all(writes).await;
        let succeeded = outcomes.iter().filter(|o| o.error.is_none()).count();
        let total = outcomes.len();

        if succeeded >= self.min_acks {
            Ok(())
        } else {
            Err(MultiRegionError {
                succeeded,
                total,
                min_acks: self.min_acks,
                failures: outcomes.into_iter().filter(|o| o.error.is_some()).collect(),
            })
        }
    }
}

/// `futures`クレートを新規依存させないための最小限の`join_all`実装
/// (全future完了まで待ち、結果をVecにまとめるだけの用途に限定)。
async fn futures_join_all<F, T>(iter: impl IntoIterator<Item = F>) -> Vec<T>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handles: Vec<_> = iter.into_iter().map(tokio::spawn).collect();
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        // spawnしたタスクがpanicした場合はpropagateする(呼び出し元の
        // バグを握りつぶさないため、ここでは`expect`する設計)。
        out.push(h.await.expect("region write task panicked"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_web_server_core::IdempotencyKey;

    fn temp_sqlite_path(label: &str) -> String {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "open-web-server-ledger-region-{label}-{}-{}.jsonl",
            std::process::id(),
            label
        ));
        let _ = std::fs::remove_file(&p);
        p.to_string_lossy().replace('\\', "/")
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

    #[tokio::test]
    async fn happy_path_writes_land_in_every_real_file_region() {
        let path_a = temp_sqlite_path("a");
        let path_b = temp_sqlite_path("b");
        let region_a = Region::connect("region-a", &path_a).await.unwrap();
        let region_b = Region::connect("region-b", &path_b).await.unwrap();

        let replicator = MultiRegionReplicator::new(vec![region_a, region_b]);
        let req = sample_request("mr-happy-1");

        replicator.replicate(&req).await.expect("all regions should succeed");

        for name in ["region-a", "region-b"] {
            let region = replicator.region(name).unwrap();
            let stored = region
                .get("mr-happy-1")
                .await
                .unwrap()
                .expect("value must actually be present in this region's real region file");
            assert!(stored.contains("sword"));
        }

        let _ = std::fs::remove_file(&path_a);
        let _ = std::fs::remove_file(&path_b);
    }

    #[tokio::test]
    async fn default_policy_fails_the_whole_commit_when_one_region_is_unreachable() {
        let path_a = temp_sqlite_path("c");
        let region_a = Region::connect("region-a", &path_a).await.unwrap();

        // Simulate a realistic mid-flight region failure (disk full, table
        // dropped out from under it, permissions revoked, etc.) concretely
        // and honestly: connect a real sqlite pool, then drop its table so
        // the next INSERT genuinely fails against a real region file.
        let path_b_flaky = temp_sqlite_path("d");
        let region_b_flaky = Region::connect("region-b", &path_b_flaky).await.unwrap();
        region_b_flaky.break_for_test();

        let replicator = MultiRegionReplicator::new(vec![region_a, region_b_flaky]);
        let req = sample_request("mr-fail-1");

        let err = replicator
            .replicate(&req)
            .await
            .expect_err("default policy (min_acks = regions.len()) must fail the whole commit");
        assert_eq!(err.succeeded, 1);
        assert_eq!(err.total, 2);
        assert_eq!(err.min_acks, 2);
        assert_eq!(err.failures.len(), 1);
        assert_eq!(err.failures[0].region, "region-b");

        // The region that DID succeed keeps its write (no cross-region
        // rollback in this implementation, documented in the module doc).
        let region_a_ref = replicator.region("region-a").unwrap();
        assert!(region_a_ref.get("mr-fail-1").await.unwrap().is_some());

        let _ = std::fs::remove_dir_all(&path_b_flaky);
    }

    #[tokio::test]
    async fn explicit_n_of_m_degradation_tolerates_one_region_failure() {
        let path_a = temp_sqlite_path("e");
        let path_b = temp_sqlite_path("f");
        let region_a = Region::connect("region-a", &path_a).await.unwrap();
        let region_b = Region::connect("region-b", &path_b).await.unwrap();
        region_b.break_for_test();

        // Opt-in degrade to 1-of-2.
        let replicator = MultiRegionReplicator::new(vec![region_a, region_b]).with_min_acks(1);
        let req = sample_request("mr-degrade-1");

        replicator
            .replicate(&req)
            .await
            .expect("1-of-2 should be tolerated once explicitly opted into");

        let _ = std::fs::remove_file(&path_a);
        let _ = std::fs::remove_dir_all(&path_b);
    }
}
