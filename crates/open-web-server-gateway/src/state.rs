use std::sync::Arc;

use open_web_server_ledger::{DbStateReader, Ledger, LedgerConfig};

/// アプリケーション全体の共有状態。
#[derive(Clone)]
pub struct AppState {
    pub ledger: Arc<Ledger>,
    /// VersionLessAPI + Git-on-SQL ハイブリッドの読み出し側(拡張要件(1))。
    /// `Ledger`の書き込みパスとは独立したopen-runoへのHTTPクライアント
    /// (詳細は`DbStateReader`のdoc comment参照)。
    pub db_state_reader: Arc<DbStateReader>,
}

impl AppState {
    pub fn from_env() -> anyhow::Result<Self> {
        let open_runo_endpoint = std::env::var("OPEN_RUNO_ENDPOINT")
            .unwrap_or_else(|_| "https://127.0.0.1:8443".to_string());

        let config = LedgerConfig {
            open_runo_endpoint: open_runo_endpoint.clone(),
            max_retries: 5,
            retry_backoff: std::time::Duration::from_millis(200),
        };

        let wal = Arc::new(crate::handlers::wal::InMemoryWal::default());
        let ledger = Arc::new(Ledger::new(config, wal));
        let db_state_reader = DbStateReader::shared(open_runo_endpoint);

        Ok(Self { ledger, db_state_reader })
    }
}
