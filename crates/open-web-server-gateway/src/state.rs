use std::sync::Arc;

use open_web_server_ledger::{Ledger, LedgerConfig};

/// アプリケーション全体の共有状態。
#[derive(Clone)]
pub struct AppState {
    pub ledger: Arc<Ledger>,
}

impl AppState {
    pub fn from_env() -> anyhow::Result<Self> {
        let open_runo_endpoint = std::env::var("OPEN_RUNO_ENDPOINT")
            .unwrap_or_else(|_| "https://127.0.0.1:8443".to_string());

        let config = LedgerConfig {
            open_runo_endpoint,
            max_retries: 5,
            retry_backoff: std::time::Duration::from_millis(200),
        };

        let wal = Arc::new(crate::handlers::wal::InMemoryWal::default());
        let ledger = Arc::new(Ledger::new(config, wal));

        Ok(Self { ledger })
    }
}
