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

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use open_web_server_core::{CoreError, CoreResult, MutationReceipt, MutationRequest};
use tracing::{info, warn};

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
}

impl Ledger {
    pub fn new(config: LedgerConfig, wal: Arc<dyn WriteAheadLog>) -> Self {
        Self {
            config,
            wal,
            http: reqwest::Client::new(),
        }
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
}
