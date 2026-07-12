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
use sqlx::SqlitePool;

/// 各「リージョン」用の最小スキーマ。①PostgreSQL WAL (`postgres_wal.rs`)と
/// 同じ形状に揃えた (idempotency_keyに一意制約 → 冪等な再送に強い)。
pub const REGION_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS region_mutations (
    idempotency_key TEXT PRIMARY KEY,
    account_id      TEXT NOT NULL,
    target          TEXT NOT NULL,
    payload         TEXT NOT NULL,
    requested_at    TEXT NOT NULL
)
"#;

/// 1つの「リージョン」(実体は独立したSQLiteインスタンス)。
#[derive(Clone)]
pub struct Region {
    pub name: String,
    pool: SqlitePool,
}

impl Region {
    /// `sqlite_path` (例: `region-tokyo.sqlite3`) に接続し、スキーマを
    /// 用意する。`sqlite://path?mode=rwc` でファイルが無ければ新規作成する。
    pub async fn connect(name: impl Into<String>, sqlite_path: &str) -> anyhow::Result<Self> {
        let url = format!("sqlite://{sqlite_path}?mode=rwc");
        let pool = SqlitePool::connect(&url).await?;
        sqlx::query(REGION_SCHEMA_SQL).execute(&pool).await?;
        Ok(Self { name: name.into(), pool })
    }

    async fn write(&self, req: &MutationRequest) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO region_mutations (idempotency_key, account_id, target, payload, requested_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT (idempotency_key) DO NOTHING",
        )
        .bind(&req.idempotency_key.0)
        .bind(&req.account_id)
        .bind(&req.target)
        .bind(req.payload.to_string())
        .bind(req.requested_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// テスト専用: 障害注入 (e.g. `DROP TABLE`) のため生の`SqlitePool`を
    /// 露出する。本番コードから呼ぶ用途ではない。
    #[doc(hidden)]
    pub fn pool_for_test(&self) -> &SqlitePool {
        &self.pool
    }

    /// 検証用: このリージョンに実際に着地した値を読み出す。
    pub async fn get(&self, idempotency_key: &str) -> anyhow::Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT payload FROM region_mutations WHERE idempotency_key = ?1",
        )
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(payload,)| payload))
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
            "open-web-server-ledger-region-{label}-{}-{}.sqlite3",
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
    async fn happy_path_writes_land_in_every_real_sqlite_region() {
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
                .expect("value must actually be present in this region's real sqlite file");
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
        // the next INSERT genuinely fails against a real sqlite file.
        let path_b_flaky = temp_sqlite_path("d");
        let region_b_flaky = Region::connect("region-b", &path_b_flaky).await.unwrap();
        sqlx::query("DROP TABLE region_mutations")
            .execute(&region_b_flaky.pool)
            .await
            .unwrap();

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

        let _ = std::fs::remove_file(&path_b_flaky);
    }

    #[tokio::test]
    async fn explicit_n_of_m_degradation_tolerates_one_region_failure() {
        let path_a = temp_sqlite_path("e");
        let path_b = temp_sqlite_path("f");
        let region_a = Region::connect("region-a", &path_a).await.unwrap();
        let region_b = Region::connect("region-b", &path_b).await.unwrap();
        sqlx::query("DROP TABLE region_mutations")
            .execute(&region_b.pool)
            .await
            .unwrap();

        // Opt-in degrade to 1-of-2.
        let replicator = MultiRegionReplicator::new(vec![region_a, region_b]).with_min_acks(1);
        let req = sample_request("mr-degrade-1");

        replicator
            .replicate(&req)
            .await
            .expect("1-of-2 should be tolerated once explicitly opted into");

        let _ = std::fs::remove_file(&path_a);
        let _ = std::fs::remove_file(&path_b);
    }
}
