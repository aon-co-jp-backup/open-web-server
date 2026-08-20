//! `GET`/`POST /admin/auto-update` — 自己アップデート機能の実行時
//! 有効/無効切替+状態確認(`auto-update` feature、2026-08-19新設)。
//! 既存の`x-admin-token`/`KeyGuardian`認証パターン
//! (`handlers::tenants::check_admin_auth`)をそのまま再利用する
//! (`power_profile.rs`と同じ、小規模な自己完結機能の管理APIテンプレート)。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::handlers::tenants::check_admin_auth;
use crate::response::{json_response, read_json_body, BoxBody};
use crate::state::AppState;

#[derive(Debug, Serialize)]
struct AutoUpdateStatusResponse {
    enabled: bool,
    current_version: &'static str,
    repo: &'static str,
}

/// `GET /admin/auto-update` — 現在の有効/無効状態を返す。
pub async fn get_auto_update(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }
    json_response(
        StatusCode::OK,
        &AutoUpdateStatusResponse {
            enabled: state.auto_update.is_enabled(),
            current_version: env!("CARGO_PKG_VERSION"),
            repo: "aon-co-jp/open-web-server",
        },
    )
}

#[derive(Debug, Deserialize)]
struct SetAutoUpdateRequest {
    enabled: bool,
}

/// `POST /admin/auto-update` — 有効/無効を切り替える(再起動不要、
/// `power_profile`と同じ即時反映の設計)。
pub async fn set_auto_update(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, &req) {
        return resp;
    }

    let body: SetAutoUpdateRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    state.auto_update.set_enabled(body.enabled);
    tracing::info!(enabled = body.enabled, "auto-update toggled at runtime (no restart)");
    json_response(
        StatusCode::OK,
        &AutoUpdateStatusResponse {
            enabled: state.auto_update.is_enabled(),
            current_version: env!("CARGO_PKG_VERSION"),
            repo: "aon-co-jp/open-web-server",
        },
    )
}
