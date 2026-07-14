//! テナント(ドメイン)管理用の内部API。
//!
//! ドメイン追加/削除がプロセス再起動やポート個別割り当てを伴わないことを
//! 実際のハンドラとして示す(`tenant_router::TenantRegistry` 参照)。
//! 本番運用では認証・監査ログを追加すべきだが、第一実装としてはレジストリ
//! 操作の配線を優先する。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};

use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;
use crate::tenant_router::{TenantConfig, TenantError};

const ADMIN_TOKEN_HEADER: &str = "x-admin-token";

/// `OPEN_WEB_SERVER_ADMIN_TOKEN` が設定されている場合のみ検証する簡易認証。
///
/// 第一実装として共有シークレットのヘッダ比較のみを行う(本番運用では
/// mTLS・OAuth等への置き換えを推奨。CLAUDE.md HANDOFFに明記)。環境変数が
/// 未設定の場合は開発用途とみなし無検証で通す(既存の挙動を壊さないため)。
fn check_admin_auth(req: &Request<Incoming>) -> Result<(), Response<BoxBody>> {
    let Ok(expected) = std::env::var("OPEN_WEB_SERVER_ADMIN_TOKEN") else {
        return Ok(());
    };

    let provided = req
        .headers()
        .get(ADMIN_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok());

    match provided {
        Some(token) if token == expected => Ok(()),
        _ => Err(text_response(
            StatusCode::UNAUTHORIZED,
            format!("missing or invalid '{ADMIN_TOKEN_HEADER}' header"),
        )),
    }
}

/// `POST /admin/tenants` — ドメインを1件、無停止で追加する。
pub async fn add_tenant(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&req) {
        return resp;
    }

    let config: TenantConfig = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    match state.tenants.add(config).await {
        Ok(()) => text_response(StatusCode::CREATED, "tenant added"),
        Err(TenantError::AlreadyExists(host)) => text_response(
            StatusCode::CONFLICT,
            format!("host '{host}' is already registered"),
        ),
        Err(e) => text_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// `DELETE /admin/tenants/:host` 相当(パスの末尾セグメントをhostとして扱う)。
pub async fn remove_tenant(
    state: Arc<AppState>,
    req: &Request<Incoming>,
    host: &str,
) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(req) {
        return resp;
    }

    match state.tenants.remove(host).await {
        Ok(()) => text_response(StatusCode::OK, "tenant removed"),
        Err(TenantError::NotFound(host)) => {
            text_response(StatusCode::NOT_FOUND, format!("host '{host}' not found"))
        }
        Err(e) => text_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// `GET /admin/tenants` — 登録済みドメイン一覧。
pub async fn list_tenants(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(req) {
        return resp;
    }

    let list = state.tenants.list().await;
    json_response(StatusCode::OK, &list)
}
