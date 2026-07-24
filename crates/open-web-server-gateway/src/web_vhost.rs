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
}

fn default_php_enabled() -> bool {
    true
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
        }
    }

    pub async fn upsert(&self, config: WebVhostConfig) {
        self.vhosts
            .write()
            .await
            .insert(config.host.clone(), Arc::new(config));
    }

    pub async fn remove(&self, host: &str) -> Result<(), WebVhostError> {
        let mut guard = self.vhosts.write().await;
        guard
            .remove(host)
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

    fn sample(host: &str) -> WebVhostConfig {
        WebVhostConfig {
            host: host.to_string(),
            docroot: PathBuf::from("/var/www/example"),
            php_enabled: true,
            compat_mode: CompatMode::default(),
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
