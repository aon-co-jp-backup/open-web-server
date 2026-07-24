//! ホスト名ベースの汎用301リダイレクト管理API。
//!
//! 既存の`handlers::tenants`/`handlers::web_vhost`と同じ`x-admin-token`/
//! `KeyGuardian`認証パターン(`check_admin_auth`)を再利用する。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};

use crate::redirects::{RedirectError, RedirectRule};
use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;

/// `POST /admin/redirects` — リダイレクトルールを追加(または既存ホストを
/// 置き換え)る。
pub async fn upsert_redirect(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = crate::handlers::tenants::check_admin_auth(&state, &req) {
        return resp;
    }

    let rule: RedirectRule = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    state.redirects.upsert(rule).await;
    text_response(StatusCode::CREATED, "redirect rule registered")
}

/// `DELETE /admin/redirects/:host`
pub async fn remove_redirect(
    state: Arc<AppState>,
    req: &Request<Incoming>,
    host: &str,
) -> Response<BoxBody> {
    if let Err(resp) = crate::handlers::tenants::check_admin_auth(&state, req) {
        return resp;
    }

    match state.redirects.remove(host).await {
        Ok(()) => text_response(StatusCode::OK, "redirect rule removed"),
        Err(RedirectError::NotFound(host)) => {
            text_response(StatusCode::NOT_FOUND, format!("host '{host}' not found"))
        }
    }
}

/// `GET /admin/redirects` — 登録済みルール一覧。
pub async fn list_redirects(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = crate::handlers::tenants::check_admin_auth(&state, req) {
        return resp;
    }

    let list = state.redirects.list().await;
    json_response(StatusCode::OK, &list)
}
