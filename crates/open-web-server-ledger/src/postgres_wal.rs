//! PostgreSQL-backed `WriteAheadLog` (拡張要件(4) 「DATABASE書き込みの四重化」
//! の**①PostgreSQL**の第一実装)
//!
//! ## スコープと限界 (2026-07-12、正直な記載)
//!
//! `open-web-server/CLAUDE.md` 拡張要件(4)は、PostgreSQLを
//! 「ACIDトランザクション保証が必須の金融システムにおけるデフォルト選択」
//! として①に位置づけている。本モジュールは既存の [`crate::WriteAheadLog`]
//! トレイトに対する**実際のPostgreSQLバックエンド実装**であり、
//! `handlers/wal.rs`(コメントで「本番実装への差し替え前提」と明示されて
//! いた `InMemoryWal`)の置き換え候補として提供する。
//!
//! - **実トランザクション**: `append`/`mark_committed` はいずれも
//!   明示的な `BEGIN`(`pool.begin()`)→ 更新 → `COMMIT`(`tx.commit()`)の
//!   実トランザクション境界を持つ。`append` は
//!   `INSERT ... ON CONFLICT (idempotency_key) DO NOTHING` で二重書き込みを
//!   防ぎ、`mark_committed` は同一トランザクション内で
//!   `UPDATE ... WHERE idempotency_key = $1` を行う——中間状態が他の
//!   コネクションから見えないことをPostgreSQL自身のACID保証に委ねる。
//! - **本番運用に必要な最小スキーマ**は [`SCHEMA_SQL`] 定数として同梱し、
//!   `PostgresWal::ensure_schema` で `CREATE TABLE IF NOT EXISTS` を実行する。
//! - **検証の限界 (正直な記載)**: この開発環境 (Windowsサンドボックス) には
//!   到達可能なPostgreSQLインスタンスが無く、`docker-compose.yml` も
//!   このリポジトリには存在しない。`pg_isready` コマンドも未インストールで
//!   あることを確認済み。そのため**実DBに対する統合テストは実施できていない**
//!   ——これは正当なブロッカーであり、実行できたと偽って報告しない。
//!   代わりに、(a) SQL文字列そのものの単体テスト(クエリ構築ロジックが
//!   ライブDB接続なしで検証可能であることを保証する)、(b)
//!   `DATABASE_URL` 環境変数が設定されている場合にのみ実行される
//!   `#[ignore]` 付き統合テスト(到達可能なPostgreSQLがある環境では
//!   `cargo test -- --ignored` で実際にBEGIN/COMMITを検証できる)、の
//!   2段構えで検証可能性を確保した。

use async_trait::async_trait;
use open_web_server_core::{IdempotencyKey, MutationReceipt, MutationRequest};
use sqlx::PgPool;

use crate::WriteAheadLog;

/// 最小スキーマ。`idempotency_key` に一意制約を張ることで、
/// `INSERT ... ON CONFLICT DO NOTHING` による冪等な先行書き込みを可能にする。
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS ledger_mutations (
    idempotency_key TEXT PRIMARY KEY,
    account_id      TEXT NOT NULL,
    target          TEXT NOT NULL,
    payload         JSONB NOT NULL,
    requested_at    TIMESTAMPTZ NOT NULL,
    committed       BOOLEAN NOT NULL DEFAULT FALSE,
    db_commit_id    TEXT,
    committed_at    TIMESTAMPTZ
)
"#;

pub struct PostgresWal {
    pool: PgPool,
}

impl PostgresWal {
    /// 既存の接続プールから構築する (アプリ側でプールを共有したい場合)。
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 接続文字列からプールを作成して構築する。
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    /// スキーマが無ければ作成する (冪等)。
    pub async fn ensure_schema(&self) -> anyhow::Result<()> {
        sqlx::query(SCHEMA_SQL).execute(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
impl WriteAheadLog for PostgresWal {
    /// 実トランザクション (`BEGIN` ... `COMMIT`) で先行書き込みを行う。
    /// `ON CONFLICT DO NOTHING` により、同一 idempotency_key の再送は
    /// 新規行を作らない (冪等)。
    async fn append(&self, req: &MutationRequest) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(insert_sql())
            .bind(&req.idempotency_key.0)
            .bind(&req.account_id)
            .bind(&req.target)
            .bind(&req.payload)
            .bind(req.requested_at)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// 同じく実トランザクションで確定状態へ更新する。
    async fn mark_committed(&self, key: &str, commit_id: &str) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(mark_committed_sql())
            .bind(commit_id)
            .bind(chrono::Utc::now())
            .bind(key)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn is_already_processed(
        &self,
        key: &str,
    ) -> anyhow::Result<Option<MutationReceipt>> {
        let row: Option<(String, bool, Option<String>, Option<chrono::DateTime<chrono::Utc>>)> =
            sqlx::query_as(select_sql())
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;

        Ok(row.map(|(idempotency_key, committed, db_commit_id, committed_at)| MutationReceipt {
            idempotency_key: IdempotencyKey(idempotency_key),
            committed,
            db_commit_id,
            committed_at,
        }))
    }
}

/// クエリ構築ロジック本体。ライブDB接続なしで文字列を検証できるよう、
/// `sqlx::query`/`query_as` に渡す前段で関数として切り出している。
fn insert_sql() -> &'static str {
    "INSERT INTO ledger_mutations \
     (idempotency_key, account_id, target, payload, requested_at) \
     VALUES ($1, $2, $3, $4, $5) \
     ON CONFLICT (idempotency_key) DO NOTHING"
}

fn mark_committed_sql() -> &'static str {
    "UPDATE ledger_mutations \
     SET committed = TRUE, db_commit_id = $1, committed_at = $2 \
     WHERE idempotency_key = $3"
}

fn select_sql() -> &'static str {
    "SELECT idempotency_key, committed, db_commit_id, committed_at \
     FROM ledger_mutations WHERE idempotency_key = $1"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ライブDB接続なしで検証できるクエリ構築ロジックのテスト。
    /// SQL文字列そのものの正しさ(バインドパラメータ数・ON CONFLICT句・
    /// テーブル名の一致)を保証する。
    #[test]
    fn insert_sql_has_on_conflict_do_nothing_for_idempotency() {
        let sql = insert_sql();
        assert!(sql.contains("ON CONFLICT (idempotency_key) DO NOTHING"));
        assert_eq!(sql.matches('$').count(), 5, "expects 5 bind parameters");
    }

    #[test]
    fn mark_committed_sql_updates_by_idempotency_key() {
        let sql = mark_committed_sql();
        assert!(sql.contains("WHERE idempotency_key = $3"));
        assert!(sql.contains("SET committed = TRUE"));
    }

    #[test]
    fn select_sql_targets_ledger_mutations_table() {
        let sql = select_sql();
        assert!(sql.contains("FROM ledger_mutations"));
        assert!(sql.contains("WHERE idempotency_key = $1"));
    }

    #[test]
    fn schema_sql_declares_primary_key_and_conflict_target_match() {
        // insert_sql の ON CONFLICT ターゲットと、実スキーマのPRIMARY KEY列が
        // 一致していることを保証する (不一致だとON CONFLICTが実行時エラーになる)。
        assert!(SCHEMA_SQL.contains("idempotency_key TEXT PRIMARY KEY"));
        assert!(insert_sql().contains("ON CONFLICT (idempotency_key)"));
    }

    /// 実PostgreSQLに対する統合テスト。`DATABASE_URL` 環境変数が設定されて
    /// いる環境でのみ意味を持つため `#[ignore]` にしてある
    /// (`cargo test -p open-web-server-ledger -- --ignored` で実行)。
    /// このサンドボックス環境には到達可能なPostgreSQLが無いため、
    /// このテスト自体は今回のパスでは実行できていない (正直な記載)。
    #[tokio::test]
    #[ignore = "requires a live PostgreSQL reachable via DATABASE_URL; not available in this sandbox"]
    async fn live_postgres_append_and_commit_round_trip() {
        let database_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set to run this ignored integration test");
        let wal = PostgresWal::connect(&database_url).await.unwrap();
        wal.ensure_schema().await.unwrap();

        let req = MutationRequest {
            idempotency_key: IdempotencyKey(format!(
                "pg-test-{}",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
            )),
            account_id: "user-pg-1".to_string(),
            target: "items".to_string(),
            payload: serde_json::json!({"item_id": "shield", "quantity": 1}),
            requested_at: chrono::Utc::now(),
        };

        wal.append(&req).await.unwrap();
        wal.mark_committed(&req.idempotency_key.0, "commit-pg-1")
            .await
            .unwrap();

        let receipt = wal
            .is_already_processed(&req.idempotency_key.0)
            .await
            .unwrap()
            .expect("row must exist after append+commit");
        assert!(receipt.committed);
        assert_eq!(receipt.db_commit_id.as_deref(), Some("commit-pg-1"));
    }
}
