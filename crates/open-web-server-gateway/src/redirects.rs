//! ホスト名ベースの汎用301リダイレクト機能。
//!
//! nginxの`server_name www.example.com; return 301
//! https://example.com$request_uri;`に相当する、`open-web-server`単体で
//! 完結する実装(2026-07-24追記、www→裸ドメインのようなリダイレクトを
//! nginx無しで再現するため)。
//!
//! `RedirectRegistry::resolve()`はHostヘッダ(ポート番号は除去)のみで
//! 引き、該当ルールが見つかれば`redirect_to`(スキーム込みの完全な
//! ベースURL)+元リクエストのパス+クエリ(`$request_uri`相当)を連結した
//! `Location`ヘッダ付き301レスポンスを組み立てる。この機能は「該当ホストは
//! 常にリダイレクトのみ」という性質上、`main::dispatch()`の他のどの
//! ハンドラより先にチェックされる(`tenant_router`/`web_vhost`より優先)。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use http_body_util::Full;
use hyper::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::response::BoxBody;

/// 1件のリダイレクトルール。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedirectRule {
    /// リダイレクト元のHostヘッダ値(例: "www.audiocafe.tokyo")。
    pub host: String,
    /// リダイレクト先のベースURL(スキーム込み、例:
    /// "https://audiocafe.tokyo")。末尾スラッシュの有無は問わない
    /// (`resolve`側で正規化する)。元リクエストのパス+クエリ
    /// (`$request_uri`相当)はこのベースURLへ自動的に付与される。
    pub redirect_to: String,
}

/// `redirects.toml`の直列化用ラッパー(`domains.toml`/`web_vhosts.toml`と
/// 同じ作法)。
#[derive(Serialize, Deserialize, Default)]
struct RedirectsFile {
    #[serde(rename = "redirect", default)]
    redirects: Vec<RedirectRule>,
}

/// ホスト名 → リダイレクトルールの共有レジストリ。
#[derive(Debug, Default)]
pub struct RedirectRegistry {
    rules: RwLock<HashMap<String, Arc<RedirectRule>>>,
    /// 設定済みの場合、`upsert`/`remove`のたびに現在の全ルールをこのパスへ
    /// TOMLとして書き戻す(`tenant_router::TenantRegistry`と同じ
    /// persist_path方式、2026-07-29追加)。
    persist_path: RwLock<Option<PathBuf>>,
}

#[derive(Debug, thiserror::Error)]
pub enum RedirectError {
    #[error("host '{0}' is not registered")]
    NotFound(String),
}

impl RedirectRegistry {
    pub fn new() -> Self {
        Self {
            rules: RwLock::new(HashMap::new()),
            persist_path: RwLock::new(None),
        }
    }

    /// 以後の`upsert`/`remove`を、指定パスのTOMLファイルへ自動的に
    /// 書き戻すようにする(`OPEN_WEB_SERVER_REDIRECTS_FILE`起動時ロードと
    /// 対にして使う想定)。
    pub async fn set_persist_path(&self, path: PathBuf) {
        *self.persist_path.write().await = Some(path);
    }

    /// 現在のルール一覧を、設定済みの永続化パスへ原子的に(一時ファイル→
    /// rename)書き戻す。パス未設定なら何もしない。書き込み失敗は呼び出し
    /// 元のupsert/remove自体を失敗させない(可用性優先、警告ログのみ)。
    async fn persist(&self, rules: &HashMap<String, Arc<RedirectRule>>) {
        let Some(path) = self.persist_path.read().await.clone() else {
            return;
        };

        let file = RedirectsFile {
            redirects: rules.values().map(|v| (**v).clone()).collect(),
        };

        let toml_str = match toml::to_string_pretty(&file) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "failed to serialize redirects.toml for persistence");
                return;
            }
        };

        let tmp_path = path.with_extension("toml.tmp");
        if let Err(e) = tokio::fs::write(&tmp_path, toml_str).await {
            tracing::warn!(error = %e, path = %tmp_path.display(), "failed to write redirects.toml tmp file");
            return;
        }
        if let Err(e) = tokio::fs::rename(&tmp_path, &path).await {
            tracing::warn!(error = %e, path = %path.display(), "failed to persist redirects.toml (rename)");
        }
    }

    /// ルールを追加(または既存ホストを置き換え)る。
    pub async fn upsert(&self, rule: RedirectRule) {
        let mut guard = self.rules.write().await;
        guard.insert(rule.host.clone(), Arc::new(rule));
        self.persist(&guard).await;
    }

    pub async fn remove(&self, host: &str) -> Result<(), RedirectError> {
        let mut guard = self.rules.write().await;
        let removed = guard.remove(host);
        if removed.is_some() {
            self.persist(&guard).await;
        }
        removed
            .map(|_| ())
            .ok_or_else(|| RedirectError::NotFound(host.to_string()))
    }

    /// Hostヘッダ(ポート番号があれば除去)からルールを引く。
    pub async fn resolve(&self, host_header: &str) -> Option<Arc<RedirectRule>> {
        let host = host_header.split(':').next().unwrap_or(host_header);
        self.rules.read().await.get(host).cloned()
    }

    pub async fn list(&self) -> Vec<RedirectRule> {
        self.rules
            .read()
            .await
            .values()
            .map(|v| (**v).clone())
            .collect()
    }

    pub async fn len(&self) -> usize {
        self.rules.read().await.len()
    }

    /// `redirects.toml`相当のTOML文字列から一括ロードする。
    pub async fn load_from_toml(&self, toml_str: &str) -> anyhow::Result<usize> {
        let parsed: RedirectsFile = toml::from_str(toml_str)?;
        let mut guard = self.rules.write().await;
        let count = parsed.redirects.len();
        for rule in parsed.redirects {
            guard.insert(rule.host.clone(), Arc::new(rule));
        }
        Ok(count)
    }
}

/// リクエストのパス+クエリ(`$request_uri`相当)を`rule.redirect_to`へ
/// 連結し、`Location`ヘッダ付きの301レスポンスを組み立てる。
pub fn build_redirect_response(rule: &RedirectRule, path_and_query: &str) -> Response<BoxBody> {
    let base = rule.redirect_to.trim_end_matches('/');
    let suffix = if path_and_query.starts_with('/') {
        path_and_query.to_string()
    } else {
        format!("/{path_and_query}")
    };
    let location = format!("{base}{suffix}");

    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(hyper::header::LOCATION, location)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(bytes::Bytes::from_static(
            b"301 Moved Permanently",
        )))
        .expect("static redirect response is always well-formed")
}

/// `OPEN_WEB_SERVER_REDIRECTS_FILE`環境変数で指定された`redirects.toml`が
/// あれば起動時に一括ロードする。
pub async fn load_redirects_from_env(registry: &RedirectRegistry) -> anyhow::Result<()> {
    let Ok(path) = std::env::var("OPEN_WEB_SERVER_REDIRECTS_FILE") else {
        return Ok(());
    };
    let toml_str = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("failed to read redirects file '{path}': {e}"))?;
    let count = registry.load_from_toml(&toml_str).await?;
    registry.set_persist_path(std::path::PathBuf::from(&path)).await;
    tracing::info!(count, path, "loaded host redirect rules from file");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(host: &str, redirect_to: &str) -> RedirectRule {
        RedirectRule {
            host: host.to_string(),
            redirect_to: redirect_to.to_string(),
        }
    }

    #[tokio::test]
    async fn upsert_and_resolve() {
        let registry = RedirectRegistry::new();
        registry
            .upsert(sample("www.audiocafe.tokyo", "https://audiocafe.tokyo"))
            .await;

        let resolved = registry.resolve("www.audiocafe.tokyo").await;
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().redirect_to, "https://audiocafe.tokyo");
    }

    #[tokio::test]
    async fn resolve_strips_port() {
        let registry = RedirectRegistry::new();
        registry
            .upsert(sample("www.audiocafe.tokyo", "https://audiocafe.tokyo"))
            .await;
        assert!(registry.resolve("www.audiocafe.tokyo:8080").await.is_some());
    }

    #[tokio::test]
    async fn resolve_unknown_is_none() {
        let registry = RedirectRegistry::new();
        assert!(registry.resolve("unknown.example.com").await.is_none());
    }

    #[tokio::test]
    async fn remove_missing_fails() {
        let registry = RedirectRegistry::new();
        let err = registry.remove("nope.example.com").await.unwrap_err();
        assert!(matches!(err, RedirectError::NotFound(_)));
    }

    #[tokio::test]
    async fn remove_then_resolve_is_none() {
        let registry = RedirectRegistry::new();
        registry
            .upsert(sample("www.audiocafe.tokyo", "https://audiocafe.tokyo"))
            .await;
        registry.remove("www.audiocafe.tokyo").await.unwrap();
        assert!(registry.resolve("www.audiocafe.tokyo").await.is_none());
    }

    #[tokio::test]
    async fn load_from_toml_bulk_provisioning() {
        let registry = RedirectRegistry::new();
        let toml_str = r#"
            [[redirect]]
            host = "www.audiocafe.tokyo"
            redirect_to = "https://audiocafe.tokyo"

            [[redirect]]
            host = "www.aruaru.tokyo"
            redirect_to = "https://aruaru.tokyo"
        "#;

        let count = registry.load_from_toml(toml_str).await.unwrap();
        assert_eq!(count, 2);
        assert_eq!(registry.len().await, 2);
        assert!(registry.resolve("www.audiocafe.tokyo").await.is_some());
        assert!(registry.resolve("www.aruaru.tokyo").await.is_some());
    }

    #[test]
    fn build_redirect_response_appends_path_and_query() {
        let rule = sample("www.audiocafe.tokyo", "https://audiocafe.tokyo");
        let resp = build_redirect_response(&rule, "/foo/bar?x=1");
        assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            resp.headers().get(hyper::header::LOCATION).unwrap(),
            "https://audiocafe.tokyo/foo/bar?x=1"
        );
    }

    #[test]
    fn build_redirect_response_handles_trailing_slash_in_redirect_to() {
        let rule = sample("www.audiocafe.tokyo", "https://audiocafe.tokyo/");
        let resp = build_redirect_response(&rule, "/");
        assert_eq!(
            resp.headers().get(hyper::header::LOCATION).unwrap(),
            "https://audiocafe.tokyo/"
        );
    }

    #[test]
    fn build_redirect_response_root_path() {
        let rule = sample("www.audiocafe.tokyo", "https://audiocafe.tokyo");
        let resp = build_redirect_response(&rule, "/");
        assert_eq!(
            resp.headers().get(hyper::header::LOCATION).unwrap(),
            "https://audiocafe.tokyo/"
        );
    }
}
