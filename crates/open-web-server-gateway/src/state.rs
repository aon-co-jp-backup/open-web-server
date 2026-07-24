use std::sync::Arc;

use open_web_server_ledger::{DbStateReader, Ledger, LedgerConfig};
use open_web_server_wire::{AccelBackend, TenantCertResolver};

use crate::access_log::{AccessLogConfig, AccessLogger};
use crate::acme::ChallengeStore;
use crate::free_domain::DomainRegistry;
use crate::keyring::{GuardianConfig, KeyGuardian};
use crate::php_server::PhpServerPool;
use crate::redirects::RedirectRegistry;
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
    /// ホスト名ベースの汎用301リダイレクトルールレジストリ(`redirects`
    /// 参照、2026-07-24追記)。`tenants`/`web_vhosts`いずれとも独立した
    /// 設定空間で、`main::dispatch()`の他のどのハンドラより先に評価される。
    pub redirects: Arc<RedirectRegistry>,
    /// PHPビルトインサーバのサブプロセスプール(`php_server`参照)。
    pub php_pool: Arc<PhpServerPool>,
    /// 無料DDNS(DuckDNS)ドメインの動的レジストリ(最大`MAX_DUCKDNS_DOMAINS`
    /// 件、`free_domain`参照)。
    pub free_domains: Arc<DomainRegistry>,
    /// 構造化アクセスログ(JSON Lines + サイズローテーション、`access_log`
    /// 参照)。`OPEN_WEB_SERVER_ACCESS_LOG_PATH`未設定なら`None`(既定無効、
    /// 既存の`tracing`ベースのリクエストログとは独立して並存する)。
    pub access_logger: Option<Arc<AccessLogger>>,
    /// ペイロード変換(圧縮+暗号化)のハードウェアアクセラレータ選択
    /// (`OPEN_WEB_SERVER_ACCEL_BACKEND`環境変数、既定は`Cpu`)。Android版の
    /// 「常時電源接続版(ハードウェアアクセラレーター対応)」プロファイルは
    /// この環境変数に`gpu`/`npu`/`hardware_accelerator`を渡して起動し、
    /// 「省電力版」/「通常版」は`cpu`(または未設定)で起動する
    /// (2026-07-24、ユーザー指示)。`Cpu`以外は`open_web_server_wire::accel`
    /// が未実装のためCpuへ安全にフォールバックする(既存方針通り、
    /// 存在しない能力を実装済みと偽らない)。
    pub accel_backend: AccelBackend,
}

/// `OPEN_WEB_SERVER_ACCEL_BACKEND`環境変数の文字列表現をパースする。
/// 未知の値・未設定は`Cpu`(最も安全な既定)にフォールバックする。
fn accel_backend_from_env() -> AccelBackend {
    match std::env::var("OPEN_WEB_SERVER_ACCEL_BACKEND")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "gpu" => AccelBackend::Gpu,
        "npu" => AccelBackend::Npu,
        "hardware_accelerator" | "hw" | "hwaccel" => AccelBackend::HardwareAccelerator,
        _ => AccelBackend::Cpu,
    }
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
        let redirects = Arc::new(RedirectRegistry::new());
        let php_pool = Arc::new(PhpServerPool::from_env());
        let free_domains = Arc::new(DomainRegistry::new());
        let access_logger = AccessLogConfig::from_env().map(|cfg| Arc::new(AccessLogger::new(cfg)));
        let accel_backend = accel_backend_from_env();
        tracing::info!(?accel_backend, "payload accelerator backend resolved from OPEN_WEB_SERVER_ACCEL_BACKEND");

        Ok(Self {
            ledger,
            db_state_reader,
            tenants,
            tls_resolver,
            acme_challenges,
            keyring,
            web_vhosts,
            redirects,
            php_pool,
            free_domains,
            access_logger,
            accel_backend,
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

    /// `OPEN_WEB_SERVER_REDIRECTS_FILE`で指定された`redirects.toml`が
    /// あれば起動時に一括ロードする(ホスト名ベースの汎用301リダイレクト
    /// ルール、2026-07-24追記)。
    pub async fn load_redirects_from_env(&self) -> anyhow::Result<()> {
        crate::redirects::load_redirects_from_env(&self.redirects).await
    }
}

#[cfg(test)]
mod accel_backend_env_tests {
    use super::*;

    /// `OPEN_WEB_SERVER_ACCEL_BACKEND`はプロセス全体のグローバル環境変数
    /// のため、他のテストと並行実行されると競合し得る。Android側の3電源
    /// プロファイル(常時電源接続=hw accel希望/省電力・通常=cpu)がこの
    /// 環境変数経由でRust側へ伝わることを保証する回帰テスト。
    static ACCEL_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn unset_or_unknown_falls_back_to_cpu() {
        let _guard = ACCEL_ENV_LOCK.lock().unwrap();
        std::env::remove_var("OPEN_WEB_SERVER_ACCEL_BACKEND");
        assert_eq!(accel_backend_from_env(), AccelBackend::Cpu);

        std::env::set_var("OPEN_WEB_SERVER_ACCEL_BACKEND", "quantum");
        assert_eq!(accel_backend_from_env(), AccelBackend::Cpu);
        std::env::remove_var("OPEN_WEB_SERVER_ACCEL_BACKEND");
    }

    #[test]
    fn recognizes_gpu_npu_and_hardware_accelerator_case_insensitively() {
        let _guard = ACCEL_ENV_LOCK.lock().unwrap();

        std::env::set_var("OPEN_WEB_SERVER_ACCEL_BACKEND", "GPU");
        assert_eq!(accel_backend_from_env(), AccelBackend::Gpu);

        std::env::set_var("OPEN_WEB_SERVER_ACCEL_BACKEND", "npu");
        assert_eq!(accel_backend_from_env(), AccelBackend::Npu);

        std::env::set_var("OPEN_WEB_SERVER_ACCEL_BACKEND", "hardware_accelerator");
        assert_eq!(accel_backend_from_env(), AccelBackend::HardwareAccelerator);

        std::env::remove_var("OPEN_WEB_SERVER_ACCEL_BACKEND");
    }
}
