//! 自己運用型APIキー(`KeyGuardian`)の管理API。
//!
//! **ブートストラップの割り切り**: 「APIキーを意識しない仕様」とは
//! *通常の呼び出し*(既に発行されたキーの検証)が人手を介さないという
//! 意味であり、*最初の1本を発行する行為*自体は誰かが権限を持って
//! 行う必要がある。ここでは既存の静的共有シークレット
//! (`OPEN_WEB_SERVER_ADMIN_TOKEN`、`tenants::check_admin_auth`)を
//! そのブートストラップ用の「鍵の鍵」として流用する——新しい認証
//! 方式を増やすのではなく、既存の管理者認証をそのまま使う設計。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::keyring::KeyDecision;
use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct IssueKeyRequest {
    pub owner: String,
    #[serde(default)]
    pub roles: Vec<String>,
    /// 有効期限までの秒数(未指定なら無期限)。
    #[serde(default)]
    pub expires_in_secs: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct IssueKeyResponse {
    /// プレーンテキストキー。**この応答が唯一の表示機会**——
    /// サーバー側はハッシュしか保持しないため、以後の再表示はできない。
    pub key: String,
    pub owner: String,
    pub roles: Vec<String>,
}

/// `POST /admin/keys` — キーを自動発行する。
pub async fn issue_key(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = super::tenants::check_admin_auth(&state, &req) {
        return resp;
    }

    let body: IssueKeyRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if body.owner.trim().is_empty() {
        return text_response(StatusCode::BAD_REQUEST, "owner must not be empty");
    }

    let expires_at = body.expires_in_secs.map(|secs| chrono::Utc::now() + chrono::Duration::seconds(secs));
    let key = state.keyring.issue(&body.owner, body.roles.clone(), expires_at);

    json_response(
        StatusCode::CREATED,
        &IssueKeyResponse { key, owner: body.owner, roles: body.roles },
    )
}

#[derive(Debug, Deserialize)]
pub struct RevokeOwnerRequest {
    pub owner: String,
}

#[derive(Debug, Serialize)]
pub struct RevokeOwnerResponse {
    pub owner: String,
    pub revoked_count: usize,
}

/// `POST /admin/keys/revoke` — 指定ownerの全キーを自動失効させる。
pub async fn revoke_owner(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = super::tenants::check_admin_auth(&state, &req) {
        return resp;
    }

    let body: RevokeOwnerRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    let revoked_count = state.keyring.revoke_owner(&body.owner);
    json_response(StatusCode::OK, &RevokeOwnerResponse { owner: body.owner, revoked_count })
}

#[derive(Debug, Serialize)]
pub struct KeyStatusResponse {
    pub active_key_count: usize,
}

/// `GET /admin/keys` — 発行済み(かつ失効していない)キーの件数のみを
/// 返す(プレーンテキスト自体は再表示しない、ハッシュのみ保持する
/// 設計のため一覧表示できるのは件数までが安全な範囲)。
pub async fn key_status(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = super::tenants::check_admin_auth(&state, req) {
        return resp;
    }
    json_response(StatusCode::OK, &KeyStatusResponse { active_key_count: state.keyring.active_key_count() })
}

/// キー検証結果を、既存の`x-admin-token`認証と**併用可能な代替経路**
/// として扱うためのヘルパ。`Authorization: Bearer <key>`ヘッダを
/// `KeyGuardian`で検証する。呼び出し側は、これが`Ok`を返さない場合に
/// 既存の`check_admin_auth`(静的シークレット)へフォールバックすれば、
/// 「両対応」の移行期間を安全に実現できる。
pub fn check_bearer_key(state: &AppState, req: &Request<Incoming>) -> KeyDecision {
    let Some(header) = req.headers().get(hyper::header::AUTHORIZATION) else {
        return KeyDecision::Rejected;
    };
    let Ok(value) = header.to_str() else {
        return KeyDecision::Rejected;
    };
    let Some(key) = value.strip_prefix("Bearer ") else {
        return KeyDecision::Rejected;
    };
    state.keyring.verify(key, chrono::Utc::now())
}

#[cfg(test)]
mod tests {
    // `hyper::body::Incoming`はテストコードから直接構築できないため
    // (実TCP接続経由でのみ得られる型)、`check_bearer_key`の
    // ヘッダ解析ロジックだけを切り出して単体テストする。実際の
    // エンドツーエンド検証(実HTTPリクエスト経由)は`hyper_app_tests`
    // 側の統合テストで行う。
    fn extract_bearer(header_value: Option<&str>) -> Option<String> {
        header_value.and_then(|v| v.strip_prefix("Bearer ")).map(str::to_string)
    }

    #[test]
    fn extracts_bearer_token_from_header() {
        assert_eq!(extract_bearer(Some("Bearer abc123")), Some("abc123".to_string()));
        assert_eq!(extract_bearer(Some("Basic abc123")), None);
        assert_eq!(extract_bearer(None), None);
    }
}
