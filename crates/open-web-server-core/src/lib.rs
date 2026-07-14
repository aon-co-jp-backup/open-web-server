//! open-web-server-core
//!
//! open-web-server 全体で共有するドメインモデル・エラー型・共通トレイトを定義する。
//! 課金アイテムや金融データなど「消失してはならない」書き込みを扱うため、
//! すべての書き込み操作は `IdempotencyKey` を必須とする設計にしている。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 冪等性キー。
///
/// クライアントは書き込みリクエストごとに一意なキーを発行する。
/// ネットワーク瞬断や3層構成内でのリトライが発生しても、
/// 同じキーでの再送は「二重課金・二重付与」を起こさない。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey(pub String);

impl IdempotencyKey {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for IdempotencyKey {
    fn default() -> Self {
        Self::new()
    }
}

/// 金融・課金アイテムなど「消えてはならない書き込み」1件分のリクエスト。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationRequest {
    pub idempotency_key: IdempotencyKey,
    pub account_id: String,
    /// aruaru-db 上のテーブル/コレクション名
    pub target: String,
    pub payload: serde_json::Value,
    pub requested_at: DateTime<Utc>,
}

/// 書き込み確定後の受領票。aruaru-db のコミットIDまで含めて返す。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationReceipt {
    pub idempotency_key: IdempotencyKey,
    pub committed: bool,
    /// aruaru-db の Git-on-SQL コミットハッシュ (再現・監査用)
    pub db_commit_id: Option<String>,
    pub committed_at: Option<DateTime<Utc>>,
}

/// `GET /internal/db/state/:target/:key/at/:commit_id` の応答。
///
/// VersionLessAPI + Git-on-SQL ハイブリッドの「読み出し側」——
/// `MutationRequest.target`/`account_id` が書き込み側で使うのと同じ
/// target/key の組に対し、指定コミット時点の値を返す。open-runo の
/// `GET /api/db/:table/:key/at/:commit_id`(2026-07-13実装済み)への
/// プロキシとして実装される(`open-web-server-ledger::DbStateReader`)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbStateAtCommitResponse {
    /// aruaru-db 上のテーブル/コレクション名(`MutationRequest.target`と同一の空間)
    pub target: String,
    pub key: String,
    pub commit_id: String,
    pub value: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("duplicate idempotency key: {0:?} (既に処理済み。二重書き込みを拒否)")]
    DuplicateKey(IdempotencyKey),

    #[error("upstream commit failed: {0}")]
    UpstreamCommitFailed(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("multi-region synchronous replication failed: {0}")]
    MultiRegionReplicationFailed(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
