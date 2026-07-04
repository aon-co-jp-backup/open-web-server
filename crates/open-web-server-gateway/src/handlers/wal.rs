//! WriteAheadLog の参照実装 (インメモリ)。
//!
//! 本番では sled/RocksDB などの永続ストレージ、あるいは aruaru-db 自身の
//! ローカルシャードに置き換える。ここではインターフェースの動作確認用。

use std::collections::HashMap;
use std::sync::Mutex;

use open_web_server_core::{MutationReceipt, MutationRequest};
use open_web_server_ledger::WriteAheadLog;

#[derive(Default)]
pub struct InMemoryWal {
    entries: Mutex<HashMap<String, (MutationRequest, Option<String>)>>,
}

#[async_trait::async_trait]
impl WriteAheadLog for InMemoryWal {
    async fn append(&self, req: &MutationRequest) -> anyhow::Result<()> {
        let mut entries = self.entries.lock().unwrap();
        entries.insert(req.idempotency_key.0.clone(), (req.clone(), None));
        Ok(())
    }

    async fn mark_committed(&self, key: &str, commit_id: &str) -> anyhow::Result<()> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(key) {
            entry.1 = Some(commit_id.to_string());
        }
        Ok(())
    }

    async fn is_already_processed(&self, key: &str) -> anyhow::Result<Option<MutationReceipt>> {
        let entries = self.entries.lock().unwrap();
        Ok(entries.get(key).and_then(|(req, commit_id)| {
            commit_id.as_ref().map(|c| MutationReceipt {
                idempotency_key: req.idempotency_key.clone(),
                committed: true,
                db_commit_id: Some(c.clone()),
                committed_at: Some(chrono::Utc::now()),
            })
        }))
    }
}
