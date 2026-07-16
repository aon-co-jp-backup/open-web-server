use std::sync::Arc;

use open_web_server_ledger::{DbStateReader, Ledger, LedgerConfig};
use open_web_server_wire::TenantCertResolver;

use crate::acme::ChallengeStore;
use crate::tenant_router::TenantRegistry;

/// アプリケーション全体の共有状態。
#[derive(Clone)]
pub struct AppState {
    pub ledger: Arc<Ledger>,
    /// VersionLessAPI + Git-on-SQL ハイブリッドの読み出し側(拡張要件(1))。
    /// `Ledger`の書き込みパスとは独立したopen-runoへのHTTPクライアント
    /// (詳細は`DbStateReader`のdoc comment参照)。
    pub db_state_reader: Arc<DbStateReader>,
    /// ドメイン/サブドメインごとのマルチテナントルーティングレジストリ
    /// (open-easyweb構想、`tenant_router`参照)。
    pub tenants: Arc<TenantRegistry>,
    /// SNIに応じてテナントごとに証明書を切り替えるTLSリゾルバ
    /// (open-web-serverをApache+Nginx相当の自己完結TLS終端にする第一歩、
    /// 2026-07-16)。`tenants`とは独立した登録(証明書登録とHTTPルーティング
    /// 登録は別操作、`handlers::tls`のdoc comment参照)。
    pub tls_resolver: Arc<TenantCertResolver>,
    /// ACME HTTP-01チャレンジレスポンス(`acme.rs`参照)。このプロセス
    /// 自身はACMEクライアント本体を持たない(次回フェーズ、
    /// `docs/tls-tenant.md`参照)が、外部ACMEクライアントが発行した
    /// チャレンジを配信する側は常時有効。
    pub acme_challenges: Arc<ChallengeStore>,
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
        let tenants = Arc::new(TenantRegistry::new());
        let tls_resolver = TenantCertResolver::new();
        let acme_challenges = Arc::new(ChallengeStore::new());

        Ok(Self { ledger, db_state_reader, tenants, tls_resolver, acme_challenges })
    }

    /// `OPEN_WEB_SERVER_DOMAINS_FILE` で指定された `domains.toml` があれば
    /// 起動時に一括ロードする(個別インストールの代わりに宣言的設定1本で
    /// 複数ドメインを立ち上げるための入口)。指定が無ければ何もしない
    /// (管理API `/admin/tenants` による動的追加のみで運用可能)。
    pub async fn load_domains_from_env(&self) -> anyhow::Result<()> {
        let Ok(path) = std::env::var("OPEN_WEB_SERVER_DOMAINS_FILE") else {
            return Ok(());
        };
        let toml_str = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read domains file '{path}': {e}"))?;
        let count = self.tenants.load_from_toml(&toml_str).await?;
        self.tenants.set_persist_path(std::path::PathBuf::from(&path)).await;
        tracing::info!(count, path, "loaded tenant domains from file");
        Ok(())
    }
}
