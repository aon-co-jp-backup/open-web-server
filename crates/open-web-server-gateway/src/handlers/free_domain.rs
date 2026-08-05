//! 無料DDNS(DuckDNS)ドメインの管理API群(既存の
//! `OPEN_WEB_SERVER_ADMIN_TOKEN`/`KeyGuardian`認証を再利用):
//! - `POST /admin/ddns/setup-free-domain` — ドメインを1件登録し、即座に
//!   疎通確認する。複数回呼べば最大`free_domain::MAX_DUCKDNS_DOMAINS`件まで
//!   追加登録できる。
//! - `GET /admin/ddns/domains` — 登録済みドメイン一覧(残り枠も返す)。
//! - `DELETE /admin/ddns/domains/:domain` — 登録解除(自動更新ループの対象
//!   から即座に外れる)。
//!
//! **正直な開示(スコープの境界)**: DuckDNSのアカウント自体(トークン
//! 発行)はユーザーがduckdns.orgでログイン(GitHub/Google/Reddit等の
//! OAuth)して取得する必要があり、それ自体は自動化できない——本
//! ソフトウェアが他社サービスの認証情報を代行取得することはしない
//! (既存のセキュリティ方針と整合)。「トークンさえあれば、その先は
//! 全自動」という現実的なスコープであり、このエンドポイント群は
//! **トークンを受け取った後**の登録・疎通確認・一覧管理・自動更新
//! ループへの組み込みを担う。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;
#[cfg(feature = "ddns")]
use serde::Serialize;

use crate::free_domain::{FreeDomainError, MAX_DUCKDNS_DOMAINS};
use crate::handlers::tenants::check_admin_auth;
use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SetupFreeDomainRequest {
    /// 希望サブドメイン名(`.duckdns.org`を除いた部分、例: `"myhost"`)。
    pub domain: String,
    /// duckdns.org上でユーザー自身が取得したトークン(このソフトウェアは
    /// 代行取得しない)。
    pub token: String,
    /// 指定すると、DDNS登録の疎通確認が成功した直後に**1回だけ**
    /// Let's Encrypt(HTTP-01)で実TLS証明書の取得を試みる(`acme` feature
    /// 必須、2026-08-05追加、`crate::acme::try_auto_https`参照)。DNS
    /// 伝播が間に合わず失敗した場合はレスポンスの`https_message`に
    /// 案内が入る(リトライは呼び出し元が改めて`POST /admin/tenants/
    /// :host/tls/acme`を呼ぶ想定、このフィールド自体は自動リトライしない)。
    #[serde(default)]
    pub contact_email: Option<String>,
}

#[cfg(feature = "ddns")]
#[derive(Serialize)]
pub struct SetupFreeDomainResponse {
    /// 完全なDuckDNSホスト名(例: `"myhost.duckdns.org"`)。
    pub full_hostname: String,
    /// 即時疎通確認(1回の更新試行)が成功したか。
    pub verified: bool,
    /// DuckDNS APIの生レスポンス(デバッグ・正直な開示のため、加工せず含める)。
    pub duckdns_raw_response: String,
    /// このインスタンスで現在登録済みのドメイン数(このリクエストの分を含む)。
    pub registered_count: usize,
    /// 残り登録可能件数(`MAX_DUCKDNS_DOMAINS - registered_count`)。
    pub remaining_capacity: usize,
    /// 案内メッセージ(自動更新ループへの組み込み状況・正直なスコープ説明)。
    pub message: String,
    /// `contact_email`を指定した場合のみ`Some`——HTTPS自動取得を試みたか
    /// どうか(試みていなければ`None`、`contact_email`未指定または`acme`
    /// feature無効時)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub https_ready: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub https_message: Option<String>,
}

/// `POST /admin/ddns/setup-free-domain` — ドメインを1件登録し、即時疎通確認する。
/// 複数回呼べば最大`MAX_DUCKDNS_DOMAINS`件まで追加登録できる(21件目以降は
/// 明示的な400エラーで拒否する)。
pub async fn setup_free_domain(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, &req) {
        return resp;
    }

    let payload: SetupFreeDomainRequest = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    if payload.domain.trim().is_empty() || payload.token.trim().is_empty() {
        return text_response(
            StatusCode::BAD_REQUEST,
            "both 'domain' and 'token' must be non-empty",
        );
    }

    #[cfg(feature = "ddns")]
    {
        // 先に登録(容量チェック込み)——21件目以降はここで明示的に拒否する。
        if let Err(e) = state.free_domains.register(payload.domain.clone(), payload.token.clone()).await {
            let status = match e {
                FreeDomainError::CapacityExceeded(_) => StatusCode::BAD_REQUEST,
                FreeDomainError::NotFound(_) => StatusCode::INTERNAL_SERVER_ERROR, // registerではNotFoundは起きない
            };
            return text_response(
                status,
                format!(
                    "{e} — このインスタンスは最大{MAX_DUCKDNS_DOMAINS}件までのDuckDNSドメインしか\
                     登録できません。不要なドメインを DELETE /admin/ddns/domains/:domain で\
                     削除してから再度お試しください。"
                ),
            );
        }

        let client = reqwest::Client::new();
        match crate::free_domain::update_duckdns(&client, &payload.domain, &payload.token, None)
            .await
        {
            Ok(result) => {
                state
                    .free_domains
                    .record_update_result(&payload.domain, result.ok, None, result.raw_body.clone())
                    .await;
                let full_hostname = format!("{}.duckdns.org", payload.domain);
                let registered_count = state.free_domains.len().await;
                let remaining_capacity = MAX_DUCKDNS_DOMAINS.saturating_sub(registered_count);
                let message = if result.ok {
                    format!(
                        "疎通確認に成功しました。'{full_hostname}' は自動更新ループに登録されました\
                         (このインスタンスで現在{registered_count}/{MAX_DUCKDNS_DOMAINS}件登録済み、\
                         残り{remaining_capacity}件登録可能)。5分間隔でグローバルIPの変化を検知し、\
                         登録済み全ドメインを自動更新します。なお、DuckDNSアカウント/トークンの\
                         発行自体はduckdns.orgへのユーザー自身のログインが必要であり、本ソフトウェアは\
                         それを代行しません。"
                    )
                } else {
                    format!(
                        "DuckDNS側が失敗を返しました('{full_hostname}')。トークンや\
                         サブドメイン名を再確認してください。生レスポンス: {}\
                         (登録自体は完了していますが、自動更新も同じ理由で失敗する可能性があります)",
                        result.raw_body
                    )
                };
                let status = if result.ok {
                    StatusCode::OK
                } else {
                    StatusCode::BAD_GATEWAY
                };

                let (https_ready, https_message) = if result.ok {
                    match payload.contact_email.as_deref() {
                        #[cfg(feature = "acme")]
                        Some(email) => match crate::acme::try_auto_https(&state, &full_hostname, email).await {
                            Ok(()) => (
                                Some(true),
                                Some(format!(
                                    "'{full_hostname}' のTLS証明書を取得し、即座にhttps://で応答できるように\
                                     なりました。"
                                )),
                            ),
                            Err(e) => (
                                Some(false),
                                Some(format!(
                                    "TLS証明書の自動取得は失敗しました(DNS伝播がまだ間に合っていない\
                                     可能性があります): {e} — 数分待ってから POST /admin/tenants/\
                                     {full_hostname}/tls/acme を再度呼び出してください。"
                                )),
                            ),
                        },
                        #[cfg(not(feature = "acme"))]
                        Some(_) => (
                            Some(false),
                            Some("'contact_email' was provided, but this build was compiled without the 'acme' feature; TLS was not obtained automatically.".to_string()),
                        ),
                        None => (None, None),
                    }
                } else {
                    (None, None)
                };

                json_response(
                    status,
                    &SetupFreeDomainResponse {
                        full_hostname,
                        verified: result.ok,
                        duckdns_raw_response: result.raw_body,
                        registered_count,
                        remaining_capacity,
                        message,
                        https_ready,
                        https_message,
                    },
                )
            }
            Err(e) => text_response(
                StatusCode::BAD_GATEWAY,
                format!("failed to reach DuckDNS update API: {e}"),
            ),
        }
    }
    #[cfg(not(feature = "ddns"))]
    {
        text_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "this build was compiled without the 'ddns' feature; DuckDNS integration is unavailable",
        )
    }
}

/// `GET /admin/ddns/domains` — 登録済みドメイン一覧+残り枠。
pub async fn list_domains(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }
    let domains = state.free_domains.list().await;
    let count = state.free_domains.len().await;
    let body = serde_json::json!({
        "domains": domains,
        "count": count,
        "capacity": MAX_DUCKDNS_DOMAINS,
        "remaining_capacity": MAX_DUCKDNS_DOMAINS.saturating_sub(count),
    });
    json_response(StatusCode::OK, &body)
}

/// `DELETE /admin/ddns/domains/:domain` — 登録解除。
pub async fn remove_domain(state: Arc<AppState>, req: &Request<Incoming>, domain: &str) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }
    match state.free_domains.remove(domain).await {
        Ok(()) => text_response(StatusCode::OK, format!("domain '{domain}' removed")),
        Err(FreeDomainError::NotFound(d)) => {
            text_response(StatusCode::NOT_FOUND, format!("domain '{d}' not found"))
        }
        Err(e) => text_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
