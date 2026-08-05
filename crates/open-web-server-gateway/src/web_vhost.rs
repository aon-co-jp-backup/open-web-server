//! 静的ファイル/PHPサイト向けのvhost設定(ホスト名 → docroot)。
//!
//! 既存の`tenant_router::TenantRegistry`はAPIバックエンド(open-runo /
//! poem-cosmo-tauri、`db_uri`必須)へのリバースプロキシ用途に特化して
//! いるため、静的サイト/PHPサイト(DB接続文字列を持たない、audiocafe.tokyo
//! のような既存PHPサイト)を同じ構造に無理に押し込まず、専用の軽量な
//! レジストリとして新設する。設定はこのエコシステムの慣例
//! (`runo-scan.txt`/`domains.toml.example`と同じTOML形式)に合わせる。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Apache互換モード/Nginx互換モードの切り替え(open-easy-webの「初回
/// セットアップガイド」画面のボタン選択に対応、2026-07-24追加)。
///
/// **正直な開示・スコープ**: Apache/Nginxの設定言語(`.htaccess`/
/// `nginx.conf`)そのものを解釈するわけではない——`php_enabled=false`の
/// 純粋な静的サイトに限定して、リクエストされたファイルがdocroot配下に
/// 実在しない場合の挙動を、2製品でよくある既定動作の差に合わせて
/// 切り替える最小限の実装:
/// - **Apache互換**: `.htaccess`の`FallbackResource`パターンでよく使われる
///   「見つからなければ`index.html`にフォールバック」(SPA的な挙動)。
/// - **Nginx互換**: `try_files $uri $uri/ =404;`相当の「見つからなければ
///   素直に404」(フォールバックしない厳格な挙動)。
/// PHP有効なvhostの挙動(静的アセット優先→PHPへ委譲)はモードに関わらず
/// 従来通り(過剰な機能追加を避けるため)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatMode {
    Apache,
    Nginx,
}

impl Default for CompatMode {
    fn default() -> Self {
        // 既存の`static_files::serve`の挙動(見つからなければ単純404)と
        // 完全に後方互換にするため、既定はNginx互換(フォールバック無し)とする。
        CompatMode::Nginx
    }
}

/// `php_enabled=true`時の実際の配信方式(2026-07-24追加)。
///
/// **背景・正直な開示**: 従来`php_enabled=true`は常に`php -S`(PHP
/// ビルトイン開発用サーバー)をサブプロセス起動してリバースプロキシする
/// 実装のみだった(`php_server.rs`のdoc comment参照)。しかし実際の
/// 本番運用(例: VPS上のaudiocafe.tokyo)は`root /var/www/audiocafe.tokyo`
/// + php-fpm(本番向けFastCGI常駐プロセス)という構成であり、`php -S`とは
/// 別物——`php -S`はドキュメント上も「本番運用での使用は非推奨」と
/// 明記された開発補助ツールに過ぎない。この列挙型で配信方式を選択可能に
/// し、既存の`php -S`運用は`BuiltinServer`として完全後方互換のまま残す。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum PhpMode {
    /// 既存実装(既定): `php -S`をdocrootごとにサブプロセス起動して
    /// リバースプロキシする(`php_server::PhpServerPool`参照)。
    BuiltinServer,
    /// 本番向け: 既に稼働しているphp-fpmのFastCGIソケット/アドレスへ
    /// `fastcgi-client`クレート経由で直接リクエストを渡す(サブプロセス
    /// は起動しない、既存のphp-fpmプロセスへ接続するだけ)。
    /// `fastcgi_addr`は`"127.0.0.1:9000"`のようなTCPアドレス、または
    /// Unixドメインソケットパス(例: `"/run/php/php8.3-fpm.sock"`)。
    FastCgi { fastcgi_addr: String },
}

impl Default for PhpMode {
    fn default() -> Self {
        // 既存の`php_enabled=true`の挙動(`php -S`サブプロセス)と完全に
        // 後方互換にするため、既定は`BuiltinServer`のまま。
        PhpMode::BuiltinServer
    }
}

/// 1つの静的/PHPサイトvhost設定。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebVhostConfig {
    /// 振り分け対象のHostヘッダ値(例: "audiocafe.tokyo")。
    pub host: String,
    /// このドメインのドキュメントルート(絶対パス)。
    pub docroot: PathBuf,
    /// PHP実行を許可するか。`false`なら純粋な静的サイトとして扱う
    /// (静的アセット以外のパスは404)。
    #[serde(default = "default_php_enabled")]
    pub php_enabled: bool,
    /// Apache互換/Nginx互換モード(2026-07-24追加、既定はNginx互換=
    /// 既存動作と同じ「フォールバック無しの404」)。
    #[serde(default)]
    pub compat_mode: CompatMode,
    /// `php_enabled=true`時の配信方式(2026-07-24追加、既定は既存の
    /// `php -S`ビルトインサーバー方式=完全後方互換)。
    #[serde(default)]
    pub php_mode: PhpMode,
    /// Apache `.htaccess`の`RewriteRule`相当のパスリライト/リダイレクト
    /// ルール(2026-08-03追加、`crate::rewrite`参照)。登録順に評価し
    /// 最初にマッチしたルールで確定する。既定は空(既存動作と完全後方
    /// 互換)。
    #[serde(default)]
    pub rewrite_rules: Vec<crate::rewrite::RewriteRule>,
    /// Basic認証設定(2026-08-05追加、既定`None`=既存動作と完全後方
    /// 互換。`BasicAuthConfig`のdoc参照)。
    #[serde(default)]
    pub basic_auth: Option<BasicAuthConfig>,
    /// TLS証明書パス設定(2026-08-05追加、既定`None`。`TlsCertConfig`の
    /// doc参照)。
    #[serde(default)]
    pub tls_cert: Option<TlsCertConfig>,
    /// 基本的なIP許可/拒否リスト(2026-08-05追加、既定`None`。
    /// `AccessControlConfig`のdoc参照)。
    #[serde(default)]
    pub access_control: Option<AccessControlConfig>,
}

fn default_php_enabled() -> bool {
    true
}

/// Basic認証設定(2026-08-05追加、ユーザー指示によるvhostフル構文対応の
/// スコープ拡張——Apacheの`AuthType Basic`+`AuthUserFile`、Nginxの
/// `auth_basic`+`auth_basic_user_file`から読み取る)。
///
/// **正直な開示**: 現時点ではパースして値を保持するのみで、実際の
/// リクエスト処理(`WWW-Authenticate`チャレンジの送出・`user_file`
/// (`htpasswd`形式)の読み込み・照合)への配線はまだ行っていない——
/// `handlers/web_vhost.rs::dispatch`側での認証チェック統合は次回の
/// 実装対象として残す(config_importのスコープはあくまで設定の抽出)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BasicAuthConfig {
    /// `AuthName`/`auth_basic`の引数(認証ダイアログに表示されるレルム名)。
    pub realm: String,
    /// `AuthUserFile`/`auth_basic_user_file`が指すパスワードファイルの
    /// パス(`htpasswd`形式を想定、中身の検証は行わない)。
    pub user_file: PathBuf,
}

/// TLS証明書パス設定(2026-08-05追加、Apacheの`SSLCertificateFile`/
/// `SSLCertificateKeyFile`、Nginxの`ssl_certificate`/`ssl_certificate_key`
/// から読み取る)。
///
/// **正直な開示**: 実際のTLS終端(`open-web-server-wire::
/// TenantCertResolver`)への配線は、このモジュールのスコープでは行って
/// いない——ここではvhost設定として証明書パスを保持するだけであり、
/// 実際に`open-web-server`自身のTLSリスナーへ反映するには、別途
/// `POST /admin/tenants/:host/tls`(ファイルパス版)または
/// `upsert_from_files`経由でこのパスを読み込ませる配線が必要
/// (既存実装がある場合はそちらへ繋ぐ、というユーザー指示に従い、今回は
/// 保持のみに留める——過剰な想像で「自動的にTLSへ反映される」とは
/// 主張しない)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TlsCertConfig {
    /// 証明書(チェーン)ファイルのパス。
    pub cert_path: PathBuf,
    /// 秘密鍵ファイルのパス(Apache/Nginxいずれも通常は証明書と鍵が
    /// 別ディレクティブ/別ファイルのため`Option`——鍵ディレクティブが
    /// 見つからないまま証明書だけ見つかった場合も、正直にその状態を
    /// 表現できるようにする)。
    #[serde(default)]
    pub key_path: Option<PathBuf>,
}

/// 基本的なIPアドレス許可/拒否リスト(2026-08-05追加)。
///
/// **正直な開示・スコープ**: Apacheの`<Directory>`ブロックが持つ
/// `Allow`/`Deny`/`Order`/`Require`の完全な評価順序・複雑な組み合わせ
/// (`Require all granted`等の`mod_authz_core`構文、Nginxの`deny`/`allow`
/// の複数行にわたる評価順序)は実装しない——単純に「許可リスト」
/// (`allow`)と「拒否リスト」(`deny`)の2つのIP/CIDR文字列集合を保持
/// するだけの、値の抽出に留めた最小実装。実際のアクセス制御ロジック
/// (どちらを優先するか、CIDR判定等)への配線もこのモジュールのスコープ
/// 外(値の保持のみ)。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessControlConfig {
    /// 許可するIPアドレス/CIDR文字列の一覧(例: `"192.168.1.0/24"`)。
    #[serde(default)]
    pub allow: Vec<String>,
    /// 拒否するIPアドレス/CIDR文字列の一覧。
    #[serde(default)]
    pub deny: Vec<String>,
}

/// `web_vhosts.toml`の直列化用ラッパー。
#[derive(Serialize, Deserialize, Default)]
struct WebVhostsFile {
    #[serde(rename = "webvhost", default)]
    vhosts: Vec<WebVhostConfig>,
}

/// ホスト名 → vhost設定の共有レジストリ。
#[derive(Debug, Default)]
pub struct WebVhostRegistry {
    vhosts: RwLock<HashMap<String, Arc<WebVhostConfig>>>,
    /// 設定済みの場合、`upsert`/`remove`のたびに現在の全vhostをこのパスへ
    /// TOMLとして書き戻す(`tenant_router::TenantRegistry`と同じ
    /// persist_path方式、2026-07-29追加——プロセス再起動でweb_vhostsが
    /// 消える設計ギャップの解消)。
    persist_path: RwLock<Option<PathBuf>>,
}

#[derive(Debug, thiserror::Error)]
pub enum WebVhostError {
    #[error("host '{0}' is not registered")]
    NotFound(String),
}

impl WebVhostRegistry {
    pub fn new() -> Self {
        Self {
            vhosts: RwLock::new(HashMap::new()),
            persist_path: RwLock::new(None),
        }
    }

    /// 以後の`upsert`/`remove`を、指定パスのTOMLファイルへ自動的に
    /// 書き戻すようにする(`OPEN_WEB_SERVER_WEB_VHOSTS_FILE`起動時ロードと
    /// 対にして使う想定、`tenant_router::TenantRegistry::set_persist_path`
    /// と同じ役割)。
    pub async fn set_persist_path(&self, path: PathBuf) {
        *self.persist_path.write().await = Some(path);
    }

    /// 現在のvhost一覧を、設定済みの永続化パスへ原子的に(一時ファイル→
    /// rename)書き戻す。パス未設定なら何もしない。書き込み失敗は呼び出し
    /// 元のupsert/remove自体を失敗させない(可用性優先、警告ログのみ)。
    async fn persist(&self, vhosts: &HashMap<String, Arc<WebVhostConfig>>) {
        let Some(path) = self.persist_path.read().await.clone() else {
            return;
        };

        let file = WebVhostsFile {
            vhosts: vhosts.values().map(|v| (**v).clone()).collect(),
        };

        let toml_str = match toml::to_string_pretty(&file) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "failed to serialize web_vhosts.toml for persistence");
                return;
            }
        };

        let tmp_path = path.with_extension("toml.tmp");
        if let Err(e) = tokio::fs::write(&tmp_path, toml_str).await {
            tracing::warn!(error = %e, path = %tmp_path.display(), "failed to write web_vhosts.toml tmp file");
            return;
        }
        if let Err(e) = tokio::fs::rename(&tmp_path, &path).await {
            tracing::warn!(error = %e, path = %path.display(), "failed to persist web_vhosts.toml (rename)");
        }
    }

    pub async fn upsert(&self, config: WebVhostConfig) {
        let mut guard = self.vhosts.write().await;
        guard.insert(config.host.clone(), Arc::new(config));
        self.persist(&guard).await;
    }

    pub async fn remove(&self, host: &str) -> Result<(), WebVhostError> {
        let mut guard = self.vhosts.write().await;
        let removed = guard.remove(host);
        if removed.is_some() {
            self.persist(&guard).await;
        }
        removed
            .map(|_| ())
            .ok_or_else(|| WebVhostError::NotFound(host.to_string()))
    }

    /// Hostヘッダ(ポート番号があれば除去)からvhostを引く。
    pub async fn resolve(&self, host_header: &str) -> Option<Arc<WebVhostConfig>> {
        let host = host_header.split(':').next().unwrap_or(host_header);
        self.vhosts.read().await.get(host).cloned()
    }

    pub async fn list(&self) -> Vec<WebVhostConfig> {
        self.vhosts
            .read()
            .await
            .values()
            .map(|v| (**v).clone())
            .collect()
    }

    pub async fn len(&self) -> usize {
        self.vhosts.read().await.len()
    }

    /// 既存vhostの`compat_mode`のみを変更する(2026-08-03追加、ユーザー
    /// 指示「Apache/Nginxのヴァーチャルホストプロファイルはどちらでも
    /// 読めていつでも両方対応可能に」)。
    ///
    /// 従来、`compat_mode`を変えるには`upsert`へ`docroot`/`php_enabled`
    /// 等を含む完全な`WebVhostConfig`を再送する必要があった——現在の
    /// 設定値を把握していないと誤って他のフィールドをリセットしてしまう
    /// リスクがあるため、`compat_mode`だけを安全に差し替えられる専用の
    /// 更新経路を追加した。インストール時に限らず、稼働中いつでも
    /// (`PUT /admin/web-vhosts/:host/compat-mode`経由で)切り替え可能。
    pub async fn set_compat_mode(&self, host: &str, compat_mode: CompatMode) -> Result<(), WebVhostError> {
        let mut guard = self.vhosts.write().await;
        let Some(existing) = guard.get(host) else {
            return Err(WebVhostError::NotFound(host.to_string()));
        };
        let mut updated = (**existing).clone();
        updated.compat_mode = compat_mode;
        guard.insert(host.to_string(), Arc::new(updated));
        self.persist(&guard).await;
        Ok(())
    }

    /// `web_vhosts.toml`相当のTOML文字列から一括ロードする。
    pub async fn load_from_toml(&self, toml_str: &str) -> anyhow::Result<usize> {
        let parsed: WebVhostsFile = toml::from_str(toml_str)?;
        let mut guard = self.vhosts.write().await;
        let count = parsed.vhosts.len();
        for config in parsed.vhosts {
            guard.insert(config.host.clone(), Arc::new(config));
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_after_set_persist_path_writes_toml_that_reloads_into_a_fresh_instance() {
        let dir = std::env::temp_dir().join(format!(
            "web_vhosts_persist_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("web_vhosts.toml");

        let registry = WebVhostRegistry::new();
        registry.set_persist_path(path.clone()).await;
        registry
            .upsert(WebVhostConfig {
                host: "audiocafe.tokyo".to_string(),
                docroot: PathBuf::from("/var/www/audiocafe.tokyo"),
                php_enabled: true,
                compat_mode: CompatMode::default(),
                php_mode: PhpMode::default(),
                rewrite_rules: Vec::new(),
                basic_auth: None,
                tls_cert: None,
                access_control: None,
            })
            .await;

        // このプロセス再起動を模した、独立した第二のレジストリインスタンスが
        // 永続化ファイルから同じ状態を復元できることを確認する
        // (`tenant_router`のset_persist_path往復テストと同じ検証方針)。
        let toml_str = std::fs::read_to_string(&path).unwrap();
        let reloaded = WebVhostRegistry::new();
        let count = reloaded.load_from_toml(&toml_str).await.unwrap();
        assert_eq!(count, 1);
        assert!(reloaded.resolve("audiocafe.tokyo").await.is_some());

        registry.remove("audiocafe.tokyo").await.unwrap();
        let toml_str_after_remove = std::fs::read_to_string(&path).unwrap();
        let reloaded_after_remove = WebVhostRegistry::new();
        reloaded_after_remove
            .load_from_toml(&toml_str_after_remove)
            .await
            .unwrap();
        assert!(reloaded_after_remove.resolve("audiocafe.tokyo").await.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn without_persist_path_no_file_is_written() {
        let dir = std::env::temp_dir().join(format!(
            "web_vhosts_no_persist_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("web_vhosts.toml");

        let registry = WebVhostRegistry::new();
        registry
            .upsert(WebVhostConfig {
                host: "example.com".to_string(),
                docroot: PathBuf::from("/var/www/example"),
                php_enabled: false,
                compat_mode: CompatMode::default(),
                php_mode: PhpMode::default(),
                rewrite_rules: Vec::new(),
                basic_auth: None,
                tls_cert: None,
                access_control: None,
            })
            .await;

        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn set_compat_mode_changes_only_that_field_and_persists() {
        let dir = std::env::temp_dir().join(format!(
            "web_vhosts_compat_mode_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("web_vhosts.toml");

        let registry = WebVhostRegistry::new();
        registry.set_persist_path(path.clone()).await;
        registry
            .upsert(WebVhostConfig {
                host: "audiocafe.tokyo".to_string(),
                docroot: PathBuf::from("/var/www/audiocafe.tokyo"),
                php_enabled: true,
                compat_mode: CompatMode::default(),
                php_mode: PhpMode::default(),
                rewrite_rules: Vec::new(),
                basic_auth: None,
                tls_cert: None,
                access_control: None,
            })
            .await;

        // 稼働中いつでも、docroot/php_enabledを再送せずcompat_modeだけ変更できる。
        registry.set_compat_mode("audiocafe.tokyo", CompatMode::Apache).await.unwrap();

        let updated = registry.resolve("audiocafe.tokyo").await.unwrap();
        assert_eq!(updated.compat_mode, CompatMode::Apache);
        // 他フィールドは維持されたまま。
        assert_eq!(updated.docroot, PathBuf::from("/var/www/audiocafe.tokyo"));
        assert!(updated.php_enabled);

        // 永続化ファイルにも即座に反映される(=再起動後も維持される)。
        let toml_str = std::fs::read_to_string(&path).unwrap();
        let reloaded = WebVhostRegistry::new();
        reloaded.load_from_toml(&toml_str).await.unwrap();
        assert_eq!(reloaded.resolve("audiocafe.tokyo").await.unwrap().compat_mode, CompatMode::Apache);

        // Nginx互換へ戻すことも即座にできる(いつでも両方向に切替可能)。
        registry.set_compat_mode("audiocafe.tokyo", CompatMode::Nginx).await.unwrap();
        assert_eq!(registry.resolve("audiocafe.tokyo").await.unwrap().compat_mode, CompatMode::Nginx);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn set_compat_mode_on_unknown_host_returns_not_found() {
        let registry = WebVhostRegistry::new();
        let err = registry.set_compat_mode("unknown.example", CompatMode::Apache).await.unwrap_err();
        assert!(matches!(err, WebVhostError::NotFound(h) if h == "unknown.example"));
    }
}

#[cfg(test)]
mod legacy_tests {
    use super::*;

    fn sample(host: &str) -> WebVhostConfig {
        WebVhostConfig {
            host: host.to_string(),
            docroot: PathBuf::from("/var/www/example"),
            php_enabled: true,
            compat_mode: CompatMode::default(),
            php_mode: PhpMode::default(),
            rewrite_rules: Vec::new(),
            basic_auth: None,
            tls_cert: None,
            access_control: None,
        }
    }

    #[test]
    fn compat_mode_defaults_to_nginx_for_backward_compat() {
        assert_eq!(CompatMode::default(), CompatMode::Nginx);
    }

    #[tokio::test]
    async fn load_from_toml_with_explicit_compat_mode() {
        let registry = WebVhostRegistry::new();
        let toml_str = r#"
            [[webvhost]]
            host = "apache-style.example.com"
            docroot = "/var/www/apache-style"
            php_enabled = false
            compat_mode = "apache"

            [[webvhost]]
            host = "nginx-style.example.com"
            docroot = "/var/www/nginx-style"
            php_enabled = false
            compat_mode = "nginx"
        "#;

        registry.load_from_toml(toml_str).await.unwrap();
        let apache_style = registry.resolve("apache-style.example.com").await.unwrap();
        assert_eq!(apache_style.compat_mode, CompatMode::Apache);
        let nginx_style = registry.resolve("nginx-style.example.com").await.unwrap();
        assert_eq!(nginx_style.compat_mode, CompatMode::Nginx);
    }

    #[tokio::test]
    async fn load_from_toml_without_compat_mode_defaults_to_nginx() {
        let registry = WebVhostRegistry::new();
        let toml_str = r#"
            [[webvhost]]
            host = "legacy.example.com"
            docroot = "/var/www/legacy"
        "#;

        registry.load_from_toml(toml_str).await.unwrap();
        let legacy = registry.resolve("legacy.example.com").await.unwrap();
        assert_eq!(legacy.compat_mode, CompatMode::Nginx);
        assert!(legacy.php_enabled);
    }

    /// `php_mode`未指定時は既存の`php -S`挙動(`BuiltinServer`)のまま
    /// (2026-07-24追加、後方互換の確認)。
    #[test]
    fn php_mode_defaults_to_builtin_server_for_backward_compat() {
        assert_eq!(PhpMode::default(), PhpMode::BuiltinServer);
    }

    /// `php_mode = { mode = "fast_cgi", fastcgi_addr = "..." }`をTOMLから
    /// 正しく読み込めることを確認する(本番向けphp-fpm/FastCGI直結対応、
    /// 2026-07-24追加)。
    #[tokio::test]
    async fn load_from_toml_with_fastcgi_php_mode() {
        let registry = WebVhostRegistry::new();
        let toml_str = r#"
            [[webvhost]]
            host = "audiocafe.tokyo"
            docroot = "/var/www/audiocafe.tokyo"
            php_enabled = true
            [webvhost.php_mode]
            mode = "fast_cgi"
            fastcgi_addr = "127.0.0.1:9000"
        "#;

        registry.load_from_toml(toml_str).await.unwrap();
        let vhost = registry.resolve("audiocafe.tokyo").await.unwrap();
        assert_eq!(
            vhost.php_mode,
            PhpMode::FastCgi {
                fastcgi_addr: "127.0.0.1:9000".to_string()
            }
        );
    }

    #[tokio::test]
    async fn upsert_and_resolve() {
        let registry = WebVhostRegistry::new();
        registry.upsert(sample("audiocafe.tokyo")).await;

        let resolved = registry.resolve("audiocafe.tokyo").await;
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().host, "audiocafe.tokyo");
    }

    #[tokio::test]
    async fn resolve_strips_port() {
        let registry = WebVhostRegistry::new();
        registry.upsert(sample("audiocafe.tokyo")).await;
        assert!(registry.resolve("audiocafe.tokyo:8080").await.is_some());
    }

    #[tokio::test]
    async fn resolve_unknown_is_none() {
        let registry = WebVhostRegistry::new();
        assert!(registry.resolve("unknown.example.com").await.is_none());
    }

    #[tokio::test]
    async fn remove_missing_fails() {
        let registry = WebVhostRegistry::new();
        let err = registry.remove("nope.example.com").await.unwrap_err();
        assert!(matches!(err, WebVhostError::NotFound(_)));
    }

    #[tokio::test]
    async fn load_from_toml_bulk_provisioning() {
        let registry = WebVhostRegistry::new();
        let toml_str = r#"
            [[webvhost]]
            host = "audiocafe.tokyo"
            docroot = "F:/open-runo/audiocafe.tokyo"
            php_enabled = true

            [[webvhost]]
            host = "static.example.com"
            docroot = "/var/www/static"
            php_enabled = false
        "#;

        let count = registry.load_from_toml(toml_str).await.unwrap();
        assert_eq!(count, 2);
        assert_eq!(registry.len().await, 2);
        let audiocafe = registry.resolve("audiocafe.tokyo").await.unwrap();
        assert!(audiocafe.php_enabled);
        let static_site = registry.resolve("static.example.com").await.unwrap();
        assert!(!static_site.php_enabled);
    }
}
