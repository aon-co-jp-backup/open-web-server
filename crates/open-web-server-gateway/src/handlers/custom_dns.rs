//! 自社ドメイン(`aon.co.jp`/`runo.tokyo`/`nasa.tokyo`/`icpo.tokyo`)配下への
//! 無料サブドメイン発行・自動更新の管理API群(2026-08-05配線、既存の
//! `x-admin-token`/`KeyGuardian`認証を再利用):
//! - `POST /admin/custom-dns/setup` — `{base_domain, subdomain}`を登録し、
//!   即座に疎通確認(初回のAレコード登録)する。複数回呼べば最大
//!   `custom_dns::MAX_CUSTOM_DOMAINS`件まで追加登録できる。
//! - `GET /admin/custom-dns/domains` — 登録済み一覧(残り枠も返す)。
//! - `DELETE /admin/custom-dns/domains/:fqdn` — 登録解除
//!   (DNSプロバイダ側のレコード削除も試みる、失敗してもローカル登録は
//!   解除済みのまま正直に報告する)。
//!
//! 既存の`handlers::free_domain`(DuckDNS向け)と同じ設計パターン
//! (メモリ帳簿とネットワーク呼び出しの分離、5分間隔の自動更新)を踏襲。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;

use crate::custom_dns::{self, CustomDomainError, MAX_CUSTOM_DOMAINS};
use crate::handlers::tenants::check_admin_auth;
use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SetupCustomDnsRequest {
    /// 対応ベースドメイン(`aon.co.jp`または`custom_dns::ConohaDnsProvider::
    /// SUPPORTED_BASE_DOMAINS`のいずれか)。
    pub base_domain: String,
    /// 希望サブドメイン名(例: `"blog"` → `blog.aon.co.jp`)。
    pub subdomain: String,
    /// 指定すると、Aレコード登録の疎通確認が成功した直後に**1回だけ**
    /// Let's Encrypt(HTTP-01)で実TLS証明書の取得を試みる(`acme` feature
    /// 必須、2026-08-05追加。`crate::acme::try_auto_https`のdoc comment
    /// にある「1回のみ・自動リトライ無し」という制約はここでも同じ)。
    #[serde(default)]
    pub contact_email: Option<String>,
}

/// `POST /admin/custom-dns/setup` — ドメインを1件登録し、即時疎通確認する。
pub async fn setup(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, &req) {
        return resp;
    }

    let payload: SetupCustomDnsRequest = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    if payload.base_domain.trim().is_empty() || payload.subdomain.trim().is_empty() {
        return text_response(
            StatusCode::BAD_REQUEST,
            "both 'base_domain' and 'subdomain' must be non-empty",
        );
    }

    if !custom_dns::is_supported_base_domain(&payload.base_domain) {
        return text_response(
            StatusCode::BAD_REQUEST,
            format!(
                "'{}' is not a base domain this server is configured to manage (supported: '{}', {:?})",
                payload.base_domain,
                custom_dns::ValueDomainProvider::BASE_DOMAIN,
                custom_dns::ConohaDnsProvider::SUPPORTED_BASE_DOMAINS,
            ),
        );
    }

    #[cfg(feature = "custom_domain")]
    {
        let fqdn = match state
            .custom_domains
            .register(payload.base_domain.clone(), payload.subdomain.clone())
            .await
        {
            Ok(fqdn) => fqdn,
            Err(e) => {
                let status = match e {
                    CustomDomainError::CapacityExceeded(_) => StatusCode::BAD_REQUEST,
                    CustomDomainError::NotFound(_) => StatusCode::INTERNAL_SERVER_ERROR, // registerでは起きない
                };
                return text_response(
                    status,
                    format!(
                        "{e} — 不要なドメインを DELETE /admin/custom-dns/domains/:fqdn で\
                         削除してから再度お試しください。"
                    ),
                );
            }
        };

        let provider = match custom_dns::build_provider(&payload.base_domain) {
            Ok(p) => p,
            Err(e) => {
                // ローカル登録(メモリ帳簿)は残す——資格情報を後から設定
                // すれば、次回の自動更新ループで再評価される設計のため。
                state.custom_domains.record_update_result(&fqdn, false, None, e.to_string()).await;
                return text_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("registered '{fqdn}' locally, but the DNS provider is not usable yet: {e}"),
                );
            }
        };

        let client = reqwest::Client::new();
        let ip = match custom_dns::fetch_current_ip(&client).await {
            Ok(ip) => ip,
            Err(e) => {
                state.custom_domains.record_update_result(&fqdn, false, None, e.to_string()).await;
                return text_response(
                    StatusCode::BAD_GATEWAY,
                    format!("registered '{fqdn}' locally, but failed to detect the current public IP: {e}"),
                );
            }
        };

        match provider.register_subdomain(&payload.subdomain, &ip).await {
            Ok(result) => {
                state
                    .custom_domains
                    .record_update_result(&fqdn, true, Some(ip.clone()), "registered".to_string())
                    .await;
                let count = state.custom_domains.len().await;

                let (https_ready, https_message): (Option<bool>, Option<String>) = match payload.contact_email.as_deref() {
                    #[cfg(feature = "acme")]
                    Some(email) => match crate::acme::try_auto_https(&state, &result.fqdn, email).await {
                        Ok(()) => (
                            Some(true),
                            Some(format!("'{}' のTLS証明書を取得し、即座にhttps://で応答できるようになりました。", result.fqdn)),
                        ),
                        Err(e) => (
                            Some(false),
                            Some(format!(
                                "TLS証明書の自動取得は失敗しました(DNS伝播がまだ間に合っていない\
                                 可能性があります): {e} — 数分待ってから POST /admin/tenants/{}/tls/acme \
                                 を再度呼び出してください。",
                                result.fqdn
                            )),
                        ),
                    },
                    #[cfg(not(feature = "acme"))]
                    Some(_) => (
                        Some(false),
                        Some("'contact_email' was provided, but this build was compiled without the 'acme' feature; TLS was not obtained automatically.".to_string()),
                    ),
                    None => (None, None),
                };

                json_response(
                    StatusCode::OK,
                    &serde_json::json!({
                        "fqdn": result.fqdn,
                        "ip": result.ip,
                        "verified": true,
                        "registered_count": count,
                        "remaining_capacity": MAX_CUSTOM_DOMAINS.saturating_sub(count),
                        "message": format!(
                            "'{}' へAレコードを登録しました。5分間隔でグローバルIPの変化を検知し、\
                             登録済み全ドメインを自動更新します。",
                            result.fqdn
                        ),
                        "https_ready": https_ready,
                        "https_message": https_message,
                    }),
                )
            }
            Err(e) => {
                state.custom_domains.record_update_result(&fqdn, false, Some(ip), e.to_string()).await;
                text_response(
                    StatusCode::BAD_GATEWAY,
                    format!("registered '{fqdn}' locally, but the DNS provider API call failed: {e}"),
                )
            }
        }
    }
    #[cfg(not(feature = "custom_domain"))]
    {
        text_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "this build was compiled without the 'custom_domain' feature; custom-domain DDNS is unavailable",
        )
    }
}

/// `GET /admin/custom-dns/domains` — 登録済みドメイン一覧+残り枠。
pub async fn list_domains(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }
    let domains = state.custom_domains.list().await;
    let count = state.custom_domains.len().await;
    json_response(
        StatusCode::OK,
        &serde_json::json!({
            "domains": domains,
            "count": count,
            "capacity": MAX_CUSTOM_DOMAINS,
            "remaining_capacity": MAX_CUSTOM_DOMAINS.saturating_sub(count),
        }),
    )
}

/// `DELETE /admin/custom-dns/domains/:fqdn` — 登録解除。DNSプロバイダ側の
/// レコード削除も試みるが、そちらが失敗してもローカル登録の解除自体は
/// 成功として扱う(正直に理由を本文へ含める、`200`のまま)。
/// (`base_domain`/`subdomain`は`custom_domain` feature無効時は未使用になる
/// ——`DELETE`自体はローカル帳簿の削除のみ行い、上流レコード削除は
/// スキップされるため。)
#[cfg_attr(not(feature = "custom_domain"), allow(unused_variables))]
pub async fn remove_domain(state: Arc<AppState>, req: &Request<Incoming>, fqdn: &str) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }

    let (base_domain, subdomain) = match state.custom_domains.remove(fqdn).await {
        Ok(entry) => entry,
        Err(CustomDomainError::NotFound(f)) => {
            return text_response(StatusCode::NOT_FOUND, format!("custom domain '{f}' not found"))
        }
        Err(e) => return text_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    #[cfg(feature = "custom_domain")]
    {
        match custom_dns::build_provider(&base_domain) {
            Ok(provider) => match provider.remove(&subdomain).await {
                Ok(()) => text_response(StatusCode::OK, format!("custom domain '{fqdn}' removed (local registry + DNS provider)")),
                Err(e) => text_response(
                    StatusCode::OK,
                    format!(
                        "custom domain '{fqdn}' removed from local registry, but DNS provider removal \
                         failed (the upstream record may still exist): {e}"
                    ),
                ),
            },
            Err(e) => text_response(
                StatusCode::OK,
                format!(
                    "custom domain '{fqdn}' removed from local registry, but the DNS provider is not \
                     usable to remove the upstream record: {e}"
                ),
            ),
        }
    }
    #[cfg(not(feature = "custom_domain"))]
    {
        text_response(
            StatusCode::OK,
            format!("custom domain '{fqdn}' removed from local registry only ('custom_domain' feature not built, upstream record was not touched)"),
        )
    }
}
