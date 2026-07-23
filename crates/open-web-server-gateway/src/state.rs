use std::sync::Arc;

use open_web_server_ledger::{DbStateReader, Ledger, LedgerConfig};
use open_web_server_wire::TenantCertResolver;

use crate::acme::ChallengeStore;
use crate::free_domain::DomainRegistry;
use crate::keyring::{GuardianConfig, KeyGuardian};
use crate::php_server::PhpServerPool;
use crate::tenant_router::TenantRegistry;
use crate::web_vhost::WebVhostRegistry;

/// アプリケーション全体の共有状態。
#[derive(Clone)]
pub struct AppState {
    pub ledger: Arc<Ledger>,
    /// 自己運用型APIキーレジストリ(第二のTomcat、WunderGraph Cosmo
    /// Enterprise互換の自動発行・自動失効・自動防衛、`keyring`参照)。
    pub keyring: Arc<KeyGuardian>,
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
    /// 静的ファイル/PHPサイト向けvhostレジストリ(Apache+Nginxハイブリッド
    /// 配信エンジン構想、`web_vhost`参照。既存の`tenants`(APIバックエンド
    /// 向け)とは独立した設定空間)。
    pub web_vhosts: Arc<WebVhostRegistry>,
    /// PHPビルトインサーバのサブプロセスプール(`php_server`参照)。
    pub php_pool: Arc<PhpServerPool>,
    /// 無料DDNS(DuckDNS)ドメインの動的レジストリ(最大`MAX_DUCKDNS_DOMAINS`
    /// 件、`free_domain`参照)。
    pub free_domains: Arc<DomainRegistry>,
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
        let keyring = Arc::new(KeyGuardian::load_from_disk(GuardianConfig::from_env()));
        let web_vhosts = Arc::new(WebVhostRegistry::new());
        let php_pool = Arc::new(PhpServerPool::from_env());
        let free_domains = Arc::new(DomainRegistry::new());

        Ok(Self {
            ledger,
            db_state_reader,
            tenants,
            tls_resolver,
            acme_challenges,
            keyring,
            web_vhosts,
            php_pool,
            free_domains,
        })
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

    /// `OPEN_WEB_SERVER_WEB_VHOSTS_FILE` で指定された `web_vhosts.toml` が
    /// あれば起動時に一括ロードする(静的ファイル/PHPサイト向けvhost)。
    pub async fn load_web_vhosts_from_env(&self) -> anyhow::Result<()> {
        let Ok(path) = std::env::var("OPEN_WEB_SERVER_WEB_VHOSTS_FILE") else {
            return Ok(());
        };
        let toml_str = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("failed to read web vhosts file '{path}': {e}"))?;
        let count = self.web_vhosts.load_from_toml(&toml_str).await?;
        tracing::info!(count, path, "loaded web vhosts from file");
        Ok(())
    }
}
