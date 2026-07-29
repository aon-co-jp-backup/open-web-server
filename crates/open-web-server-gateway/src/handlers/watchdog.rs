//! `GET /admin/watchdog/status` — ドメイン/URL死活監視の直近状態を返す
//! (2026-07-29追加、`crate::domain_watchdog`参照)。既存の`x-admin-token`/
//! `KeyGuardian`認証パターンをそのまま再利用する。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::handlers::tenants::check_admin_auth;
use crate::response::{json_response, read_json_body, BoxBody};
use crate::state::AppState;

#[derive(Debug, Serialize)]
struct HostHealthView {
    host: String,
    consecutive_failures: u32,
    last_checked_unix: u64,
    last_ok: bool,
    last_detail: String,
    last_recovery_action: Option<String>,
    last_ai_diagnosis: Option<String>,
}

/// `GET /admin/watchdog/status` — 監視対象全ホストの直近の死活状態一覧。
pub async fn get_watchdog_status(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }

    let mut views: Vec<HostHealthView> = state
        .watchdog
        .snapshot()
        .await
        .into_iter()
        .map(|(host, h)| HostHealthView {
            host,
            consecutive_failures: h.consecutive_failures,
            last_checked_unix: h.last_checked_unix,
            last_ok: h.last_ok,
            last_detail: h.last_detail,
            last_recovery_action: h.last_recovery_action,
            last_ai_diagnosis: h.last_ai_diagnosis,
        })
        .collect();
    views.sort_by(|a, b| a.host.cmp(&b.host));

    json_response(StatusCode::OK, &views)
}

#[derive(Debug, Deserialize)]
struct SetExpectationsRequest {
    /// このホストの`/`の本文に含まれているべき文字列一覧(ボタンの
    /// ラベル・リンク先URL等)。空配列を送ればコンテンツ確認を解除する。
    expected_substrings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExpectationsResponse {
    host: String,
    expected_substrings: Vec<String>,
}

/// `POST /admin/watchdog/expectations/:host` — 死活監視のコンテンツ確認
/// (ページ本文に含まれているべき文字列)を設定する。audiocafe.tokyoで
/// 実際に起きた「コードはpush済みだが本番に反映されておらず、期待する
/// リンクが実際には表示されていなかった」障害を、次回からは死活監視が
/// 自動検知できるようにするための入口。
pub async fn set_watchdog_expectations(state: Arc<AppState>, req: Request<Incoming>, host: &str) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, &req) {
        return resp;
    }

    let body: SetExpectationsRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    state
        .watchdog
        .set_expectations(host, body.expected_substrings.clone())
        .await;

    json_response(
        StatusCode::OK,
        &ExpectationsResponse {
            host: host.to_string(),
            expected_substrings: body.expected_substrings,
        },
    )
}

/// `GET /admin/watchdog/expectations` — 現在設定されているコンテンツ確認
/// の一覧。
pub async fn list_watchdog_expectations(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }

    let mut views: Vec<ExpectationsResponse> = state
        .watchdog
        .all_expectations()
        .await
        .into_iter()
        .map(|(host, expected_substrings)| ExpectationsResponse { host, expected_substrings })
        .collect();
    views.sort_by(|a, b| a.host.cmp(&b.host));

    json_response(StatusCode::OK, &views)
}
