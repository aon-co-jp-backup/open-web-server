//! 統一アカウント基盤(GitHub等マルチログイン)。
//!
//! `AuthProvider`トレイトでOAuthプロバイダを抽象化し、`GitHubOAuthProvider`
//! を第一実装として提供する(将来Google等を追加する場合は同トレイトを
//! 実装するだけでよい設計)。
//!
//! ## アカウント統一の設計
//!
//! `aon.co.jp`/`runo.tokyo`/将来の`nasa.tokyo`のいずれ経由でログインしても
//! 同一アカウントへアクセスできるようにするため、[`UnifiedAccountId`]を
//! `"<provider>:<provider_user_id>"`という一意キーとして扱い、
//! [`AccountRegistry`]がこのキーで1ユーザー1レコードに正規化する
//! (どのサイトからログインしても同じ`provider_user_id`が返るのは
//! OAuthプロバイダ側の保証に依拠する——GitHubの`id`はアカウントに対して
//! 不変)。
//!
//! ## 正直な開示
//!
//! GitHub OAuth App(`client_id`/`client_secret`の発行)はユーザー自身が
//! GitHub側の設定画面で行う必要があり、これも実装者(Claude)が代行取得
//! しない。環境変数(`OPEN_EASY_WEB_GITHUB_CLIENT_ID`/
//! `OPEN_EASY_WEB_GITHUB_CLIENT_SECRET`)経由で受け取るのみ。**実GitHub
//! OAuthフロー(認可コード→アクセストークン交換→ユーザー情報取得)の
//! 実接続はこのセッションでは未検証**であり、モックによるロジック検証
//! (単体テスト)に留まる。

use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Debug, thiserror::Error)]
pub enum AuthProviderError {
    #[error("required credential is not configured: {0}")]
    MissingCredential(&'static str),
    #[error("OAuth token exchange failed: {0}")]
    TokenExchangeFailed(String),
    #[error("OAuth provider API returned an unexpected response: {0}")]
    UnexpectedResponse(String),
}

/// OAuthログイン成功後にプロバイダから得られる、正規化済みのユーザー情報。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OAuthUserInfo {
    /// プロバイダ名(例: `"github"`)。
    pub provider: String,
    /// プロバイダ側のユーザーID(不変であることが前提、例: GitHubの`id`)。
    pub provider_user_id: String,
    pub login: String,
    pub email: Option<String>,
}

impl OAuthUserInfo {
    /// `"<provider>:<provider_user_id>"`形式の一意アカウントキー。
    pub fn unified_account_id(&self) -> String {
        format!("{}:{}", self.provider, self.provider_user_id)
    }
}

/// OAuthベースのログイン導線を抽象化するトレイト。GitHub以外(将来
/// Google等)を追加する場合はこのトレイトを実装するだけでよい。
#[async_trait]
pub trait AuthProvider: Send + Sync {
    fn provider_name(&self) -> &str;

    /// 認可コードをアクセストークンへ交換し、ユーザー情報を取得する。
    async fn exchange_code_for_user(&self, code: &str) -> Result<OAuthUserInfo, AuthProviderError>;
}

/// GitHub OAuth Appによるログイン。
#[derive(Debug)]
pub struct GitHubOAuthProvider {
    client_id: String,
    client_secret: String,
    #[cfg(feature = "custom_domain")]
    client: reqwest::Client,
}

impl GitHubOAuthProvider {
    pub const ENV_CLIENT_ID: &str = "OPEN_EASY_WEB_GITHUB_CLIENT_ID";
    pub const ENV_CLIENT_SECRET: &str = "OPEN_EASY_WEB_GITHUB_CLIENT_SECRET";

    /// 環境変数からOAuth App資格情報を読み込んで構築する。
    /// **client_id/client_secretの発行はユーザー自身がGitHub側で行う
    /// 必要があり、この関数は環境変数を読むだけで代行取得はしない**。
    pub fn from_env() -> Result<Self, AuthProviderError> {
        let client_id = std::env::var(Self::ENV_CLIENT_ID)
            .map_err(|_| AuthProviderError::MissingCredential(Self::ENV_CLIENT_ID))?;
        let client_secret = std::env::var(Self::ENV_CLIENT_SECRET)
            .map_err(|_| AuthProviderError::MissingCredential(Self::ENV_CLIENT_SECRET))?;
        if client_id.trim().is_empty() || client_secret.trim().is_empty() {
            return Err(AuthProviderError::MissingCredential(Self::ENV_CLIENT_ID));
        }
        Ok(Self {
            client_id,
            client_secret,
            #[cfg(feature = "custom_domain")]
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl AuthProvider for GitHubOAuthProvider {
    fn provider_name(&self) -> &str {
        "github"
    }

    #[cfg(feature = "custom_domain")]
    async fn exchange_code_for_user(&self, code: &str) -> Result<OAuthUserInfo, AuthProviderError> {
        #[derive(serde::Deserialize)]
        struct TokenResponse {
            access_token: String,
        }
        #[derive(serde::Deserialize)]
        struct GitHubUser {
            id: u64,
            login: String,
            email: Option<String>,
        }

        let token_resp = self
            .client
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[("client_id", self.client_id.as_str()), ("client_secret", self.client_secret.as_str()), ("code", code)])
            .send()
            .await
            .map_err(|e| AuthProviderError::TokenExchangeFailed(e.to_string()))?;
        if !token_resp.status().is_success() {
            return Err(AuthProviderError::TokenExchangeFailed(format!("HTTP {}", token_resp.status())));
        }
        let token: TokenResponse = token_resp
            .json()
            .await
            .map_err(|e| AuthProviderError::UnexpectedResponse(e.to_string()))?;

        let user_resp = self
            .client
            .get("https://api.github.com/user")
            .bearer_auth(&token.access_token)
            .header("User-Agent", "open-easy-web")
            .send()
            .await
            .map_err(|e| AuthProviderError::TokenExchangeFailed(e.to_string()))?;
        if !user_resp.status().is_success() {
            return Err(AuthProviderError::UnexpectedResponse(format!("HTTP {}", user_resp.status())));
        }
        let user: GitHubUser = user_resp
            .json()
            .await
            .map_err(|e| AuthProviderError::UnexpectedResponse(e.to_string()))?;

        Ok(OAuthUserInfo { provider: "github".to_string(), provider_user_id: user.id.to_string(), login: user.login, email: user.email })
    }

    #[cfg(not(feature = "custom_domain"))]
    async fn exchange_code_for_user(&self, _code: &str) -> Result<OAuthUserInfo, AuthProviderError> {
        Err(AuthProviderError::TokenExchangeFailed(
            "this build was compiled without the `custom_domain` feature (no HTTP client available)".to_string(),
        ))
    }
}

/// どのサイト(aon.co.jp/runo.tokyo/将来のnasa.tokyo)経由でログインしても
/// 同一アカウントへ到達させるための、`UnifiedAccountId`(=
/// `"<provider>:<provider_user_id>"`)キーによる正規化レジストリ。
pub struct AccountRegistry {
    accounts: RwLock<HashMap<String, OAuthUserInfo>>,
}

impl AccountRegistry {
    pub fn new() -> Self {
        Self { accounts: RwLock::new(HashMap::new()) }
    }

    /// ログイン成功後に呼ぶ。既存アカウントがあれば最新情報で上書きし、
    /// 無ければ新規作成する(いずれの場合もキーは不変な
    /// `unified_account_id()`)。
    pub async fn upsert_login(&self, info: OAuthUserInfo) -> String {
        let key = info.unified_account_id();
        self.accounts.write().await.insert(key.clone(), info);
        key
    }

    pub async fn get(&self, unified_account_id: &str) -> Option<OAuthUserInfo> {
        self.accounts.read().await.get(unified_account_id).cloned()
    }

    pub async fn len(&self) -> usize {
        self.accounts.read().await.len()
    }
}

impl Default for AccountRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockAuthProvider {
        provider: &'static str,
        canned: OAuthUserInfo,
    }

    #[async_trait]
    impl AuthProvider for MockAuthProvider {
        fn provider_name(&self) -> &str {
            self.provider
        }

        async fn exchange_code_for_user(&self, _code: &str) -> Result<OAuthUserInfo, AuthProviderError> {
            Ok(self.canned.clone())
        }
    }

    #[tokio::test]
    async fn mock_login_flow_produces_stable_unified_account_id() {
        let provider = MockAuthProvider {
            provider: "github",
            canned: OAuthUserInfo { provider: "github".to_string(), provider_user_id: "42".to_string(), login: "octocat".to_string(), email: Some("octocat@example.com".to_string()) },
        };
        let info = provider.exchange_code_for_user("dummy-code").await.unwrap();
        assert_eq!(info.unified_account_id(), "github:42");
    }

    #[tokio::test]
    async fn account_registry_normalizes_repeated_logins_from_different_sites() {
        let registry = AccountRegistry::new();
        let info_from_aon = OAuthUserInfo { provider: "github".to_string(), provider_user_id: "42".to_string(), login: "octocat".to_string(), email: None };
        let info_from_runo = OAuthUserInfo { provider: "github".to_string(), provider_user_id: "42".to_string(), login: "octocat".to_string(), email: Some("new@example.com".to_string()) };

        let key1 = registry.upsert_login(info_from_aon).await;
        let key2 = registry.upsert_login(info_from_runo).await;

        assert_eq!(key1, key2, "logging in from aon.co.jp and runo.tokyo must resolve to the same account");
        assert_eq!(registry.len().await, 1, "must not create a duplicate account row");
        let stored = registry.get(&key1).await.unwrap();
        assert_eq!(stored.email.as_deref(), Some("new@example.com"), "latest login info should win");
    }

    #[test]
    fn github_provider_from_env_reports_missing_credential_honestly() {
        std::env::remove_var(GitHubOAuthProvider::ENV_CLIENT_ID);
        std::env::remove_var(GitHubOAuthProvider::ENV_CLIENT_SECRET);
        let err = GitHubOAuthProvider::from_env().expect_err("must fail without an OAuth App configured");
        assert!(matches!(err, AuthProviderError::MissingCredential(_)));
    }
}
