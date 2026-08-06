//! v6プラス(MAP-E)環境向けIPv6(AAAA)自動更新の管理API群(既存の
//! `OPEN_WEB_SERVER_ADMIN_TOKEN`/`KeyGuardian`認証を`free_domain.rs`と
//! 同じパターンで再利用):
//! - `POST /admin/custom-domain/setup-ipv6-auto-update` — ドメイン名
//!   (現状`aon.co.jp`のみサポート)・サブドメイン名をリクエストボディで
//!   指定し、AAAA自動更新を有効化する。
//! - `GET /admin/custom-domain/ipv6-auto-update` — 登録済みエントリ一覧
//!   (直近の更新試行結果込み)。
//! - `DELETE /admin/custom-domain/ipv6-auto-update/:base_domain/:subdomain`
//!   — 登録解除。
//!
//! **正直な開示(スコープの境界)**: バリュードメインのAPIキー自体は
//! ユーザーが`OPEN_EASY_WEB_VALUE_DOMAIN_API_KEY`環境変数で設定する
//! 必要があり(`custom_dns::ValueDomainProvider::from_env`参照)、
//! このエンドポイント自体はキーを代行取得しない。実バリュードメイン
//! アカウントでの実接続はこのタスクでは未検証(モックHTTPサーバーでの
//! ロジック検証に留まる、詳細は`custom_dns_ipv6_autoupdate`モジュールの
//! テストコメント参照)。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;

use crate::custom_dns_ipv6_autoupdate::{Ipv6AutoUpdateError, MAX_IPV6_AUTO_UPDATE_ENTRIES};
use crate::handlers::tenants::check_admin_auth;
use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SetupIpv6AutoUpdateRequest {
    /// ベースドメイン名(現状`"aon.co.jp"`のみサポート、
    /// `custom_dns::ValueDomainProvider::BASE_DOMAIN`参照)。
    pub domain: String,
    /// サブドメイン名(例: `"home"` → `home.aon.co.jp`)。
    pub subdomain: String,
}

/// `POST /admin/custom-domain/setup-ipv6-auto-update` — AAAA自動更新を
/// 有効化する。複数回呼べば最大`MAX_IPV6_AUTO_UPDATE_ENTRIES`件まで
/// 追加登録できる(`free_domain::setup_free_domain`と同じ容量チェック
/// パターン)。
pub async fn setup_ipv6_auto_update(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, &req) {
        return resp;
    }

    let payload: SetupIpv6AutoUpdateRequest = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    if payload.domain.trim().is_empty() || payload.subdomain.trim().is_empty() {
        return text_response(
            StatusCode::BAD_REQUEST,
            "both 'domain' and 'subdomain' must be non-empty",
        );
    }

    match state.ipv6_auto_update.register(payload.domain.clone(), payload.subdomain.clone()).await {
        Ok(key) => {
            let registered_count = state.ipv6_auto_update.len().await;
            let remaining_capacity = MAX_IPV6_AUTO_UPDATE_ENTRIES.saturating_sub(registered_count);
            let body = serde_json::json!({
                "fqdn": key.fqdn(),
                "registered_count": registered_count,
                "remaining_capacity": remaining_capacity,
                "message": format!(
                    "'{}' はIPv6(AAAA)自動更新の対象として登録されました。90秒間隔で\
                     このマシンの現在のグローバルIPv6アドレス({}経由)の変化を検知し、\
                     変化していればバリュードメインのAAAAレコードを自動更新します。\
                     実際に更新が反映されるには、環境変数\
                     'OPEN_EASY_WEB_VALUE_DOMAIN_API_KEY' でバリュードメインの\
                     APIキーを設定しておく必要があります(このエンドポイント自体は\
                     キーを代行取得しません)。",
                    key.fqdn(),
                    "https://api6.ipify.org",
                ),
            });
            json_response(StatusCode::OK, &body)
        }
        Err(e @ Ipv6AutoUpdateError::UnsupportedBaseDomain(_)) => text_response(StatusCode::BAD_REQUEST, e.to_string()),
        Err(e @ Ipv6AutoUpdateError::CapacityExceeded(_)) => text_response(
            StatusCode::BAD_REQUEST,
            format!(
                "{e} — 不要なエントリを DELETE \
                 /admin/custom-domain/ipv6-auto-update/:base_domain/:subdomain \
                 で削除してから再度お試しください。"
            ),
        ),
        Err(e) => text_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// `GET /admin/custom-domain/ipv6-auto-update` — 登録済みエントリ一覧。
pub async fn list_ipv6_auto_update(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }
    let entries = state.ipv6_auto_update.list().await;
    let count = entries.len();
    let entries_json: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|(key, status)| {
            serde_json::json!({
                "base_domain": key.base_domain,
                "subdomain": key.subdomain,
                "fqdn": key.fqdn(),
                "last_update": status,
            })
        })
        .collect();
    let body = serde_json::json!({
        "entries": entries_json,
        "count": count,
        "capacity": MAX_IPV6_AUTO_UPDATE_ENTRIES,
        "remaining_capacity": MAX_IPV6_AUTO_UPDATE_ENTRIES.saturating_sub(count),
    });
    json_response(StatusCode::OK, &body)
}

/// `DELETE /admin/custom-domain/ipv6-auto-update/:base_domain/:subdomain`
/// — 登録解除。
pub async fn remove_ipv6_auto_update(
    state: Arc<AppState>,
    req: &Request<Incoming>,
    base_domain: &str,
    subdomain: &str,
) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }
    match state.ipv6_auto_update.remove(base_domain, subdomain).await {
        Ok(()) => text_response(
            StatusCode::OK,
            format!("entry '{subdomain}.{base_domain}' removed from IPv6 auto-update"),
        ),
        Err(e @ Ipv6AutoUpdateError::NotFound(_, _)) => text_response(StatusCode::NOT_FOUND, e.to_string()),
        Err(e) => text_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
