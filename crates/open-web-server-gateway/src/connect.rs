//! Connect の「単純呼び出し(unary)」で受ける管理API・公開APIの入口(2026-11-04新設)。
//!
//! 方針(ユーザー決定): **REST は全廃**。以前の `GET/POST/PUT/DELETE /admin/...`・`/api/v1/...`・
//! `/internal/db/state/...` はすべてここへ移し、旧経路は削除した。
//!
//! 形式は `POST /<パッケージ>.<Service>/<Method>`、`content-type: application/json`、本文は JSON オブジェクト:
//! - 管理系: `POST /open_web_server.admin.v1.Admin/<Method>`(認証は従来どおり `x-admin-token` または
//!   `Authorization: Bearer <KeyGuardianのキー>`。ログイン系の一部は認証なし)
//! - 公開API(課金アイテム・決済): `POST /open_web_server.api.v1.Api/<Method>`(`Idempotency-Key` ヘッダ必須)
//!
//! 旧REST のパスパラメータ(`:host` など)とクエリ(`?host=`, `?path_prefix=`)は、本文のフィールドへ移した
//! (`host` / `domain` / `base_domain` / `subdomain` / `path_prefix` / `target` / `key` / `commit_id`)。
//!
//! **JSON の読み込みは RS-JSON の `parse_secure`(サイズ・深さ・重複キーの上限つき)、書き出しは `rust_json::fast`**。
//! 応答: 成功は常に HTTP 200 + JSON オブジェクト(配列は `{"items":[...]}`、文字だけの応答は `{"message":"..."}` に包む)。
//! 失敗は Connect の形式 `{"code":"...","message":"..."}`(HTTP ステータスは Connect 仕様の対応表)。
//!
//! REST のまま残すのは、外部の規格や配布が形を決めるものだけ: `/healthz`・`/healthz/ddns`(ヘルスチェック)、
//! `/.well-known/acme-challenge/*`(ACME)、`/demo`(読み取り専用の紹介ページ)、静的ファイル配信、テナントへの素通し。

use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Limited};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::handlers;
use crate::response::{full_body, BoxBody};
use crate::state::AppState;

pub const ADMIN_PREFIX: &str = "/open_web_server.admin.v1.Admin/";
pub const API_PREFIX: &str = "/open_web_server.api.v1.Api/";

/// 本文の上限(PEM 証明書や Apache/Nginx 設定の取り込みを含めても十分な大きさ)。
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_DEPTH: usize = 32;

/// このパスが Connect の入口か(`true` ならゲートウェイは必ずここで応答する)。
pub fn is_connect_path(path: &str) -> bool {
    path.starts_with(ADMIN_PREFIX) || path.starts_with(API_PREFIX)
}

/// Connect のエラーコードから HTTP ステータスへ。
fn status_of(code: &str) -> StatusCode {
    match code {
        "invalid_argument" | "failed_precondition" | "out_of_range" => StatusCode::BAD_REQUEST,
        "unauthenticated" => StatusCode::UNAUTHORIZED,
        "permission_denied" => StatusCode::FORBIDDEN,
        "not_found" | "unimplemented" => StatusCode::NOT_FOUND,
        "aborted" | "already_exists" => StatusCode::CONFLICT,
        "resource_exhausted" => StatusCode::TOO_MANY_REQUESTS,
        "unavailable" => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// ハンドラが返した HTTP ステータスから Connect のコードへ。
fn code_of(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 => "invalid_argument",
        401 => "unauthenticated",
        403 => "permission_denied",
        404 => "not_found",
        405 | 501 => "unimplemented",
        409 => "already_exists",
        412 => "failed_precondition",
        413 | 429 => "resource_exhausted",
        502 | 503 | 504 => "unavailable",
        s if s >= 500 => "internal",
        _ => "unknown",
    }
}

fn reply(status: StatusCode, bytes: Vec<u8>) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(full_body(Bytes::from(bytes)))
        .expect("static response headers are always valid")
}

fn error_body(code: &str, message: &str) -> Vec<u8> {
    #[derive(Serialize)]
    struct E<'a> {
        code: &'a str,
        message: &'a str,
    }
    rust_json::fast::to_vec(&E { code, message }).unwrap_or_else(|_| b"{}".to_vec())
}

fn err(code: &str, message: &str) -> Response<BoxBody> {
    reply(status_of(code), error_body(code, message))
}

/// 旧ハンドラの応答(成功は text/JSON、失敗は text)を Connect の形へ直す。
async fn to_connect(resp: Response<BoxBody>) -> Response<BoxBody> {
    let (parts, body) = resp.into_parts();
    let bytes = match body.collect().await {
        Ok(c) => c.to_bytes(),
        Err(e) => return err("internal", &format!("failed to read handler response: {e}")),
    };
    let is_json = parts
        .headers
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    if !parts.status.is_success() {
        // 旧ハンドラの失敗応答は文字列が基本。JSON オブジェクトで `message` を持つもの
        // (例: 無料ドメインの疎通確認失敗)は、その `message` を Connect の `message` に使う。
        let raw = String::from_utf8_lossy(&bytes).into_owned();
        let msg = if is_json {
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| v.get("message").and_then(|m| m.as_str().map(str::to_string)))
                .unwrap_or(raw)
        } else {
            raw
        };
        return reply(parts.status, error_body(code_of(parts.status), msg.trim()));
    }
    if is_json {
        // 先頭の空白を飛ばして `{` ならそのまま、配列などは `items` に包む。
        let first = bytes.iter().find(|b| !b.is_ascii_whitespace()).copied();
        if first == Some(b'{') {
            return reply(StatusCode::OK, bytes.to_vec());
        }
        let mut out = br#"{"items":"#.to_vec();
        out.extend_from_slice(&bytes);
        out.push(b'}');
        return reply(StatusCode::OK, out);
    }
    let text = String::from_utf8_lossy(&bytes);
    #[derive(Serialize)]
    struct M<'a> {
        message: &'a str,
    }
    reply(StatusCode::OK, rust_json::fast::to_vec(&M { message: text.trim() }).unwrap_or_else(|_| b"{}".to_vec()))
}

/// 旧REST のパスパラメータ・クエリだったフィールド(ほかの本文のフィールドは各ハンドラが読む)。
#[derive(Default, Deserialize)]
#[serde(default)]
struct Params {
    host: Option<String>,
    domain: Option<String>,
    base_domain: Option<String>,
    subdomain: Option<String>,
    path_prefix: Option<String>,
    target: Option<String>,
    key: Option<String>,
    commit_id: Option<String>,
}

/// 本文を上限つきで読み、RS-JSON の `parse_secure`(深さ・重複キー)で検査する。
async fn read_checked(body: Incoming) -> Result<Bytes, Response<BoxBody>> {
    let bytes = match Limited::new(body, MAX_BODY_BYTES).collect().await {
        Ok(c) => c.to_bytes(),
        Err(e) => {
            let m = e.to_string();
            return Err(if m.contains("length limit") {
                err("resource_exhausted", &format!("request body exceeds {MAX_BODY_BYTES} bytes"))
            } else {
                err("invalid_argument", &format!("failed to read request body: {m}"))
            });
        }
    };
    if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(Bytes::from_static(b"{}"));
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Err(err("invalid_argument", "request body is not valid UTF-8"));
    };
    let limits = rust_json::Limits { max_bytes: MAX_BODY_BYTES, max_depth: MAX_DEPTH, reject_duplicate_keys: true };
    match rust_json::parse_secure(text, &limits) {
        Ok(v) if v.is_object() => Ok(bytes),
        Ok(_) => Err(err("invalid_argument", "request body must be a JSON object")),
        Err(e) => Err(err("invalid_argument", &format!("invalid JSON body: {e}"))),
    }
}

/// Connect の入口。Connect のパスでなければ `Err(req)` で呼び出し元へ返す(通常のルーティングが続く)。
pub async fn handle(state: Arc<AppState>, req: Request<Incoming>) -> Result<Response<BoxBody>, Request<Incoming>> {
    let path = req.uri().path().to_string();
    let (is_admin, method_name) = if let Some(m) = path.strip_prefix(ADMIN_PREFIX) {
        (true, m.to_string())
    } else if let Some(m) = path.strip_prefix(API_PREFIX) {
        (false, m.to_string())
    } else {
        return Err(req);
    };

    if req.method() != Method::POST {
        return Ok(err("unimplemented", "Connect unary calls must use POST"));
    }
    let ct_ok = req
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "application/json" || v.starts_with("application/json;")
        });
    if !ct_ok {
        return Ok(reply(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            error_body("invalid_argument", "content-type must be application/json"),
        ));
    }

    let (parts, body) = req.into_parts();
    let bytes = match read_checked(body).await {
        Ok(b) => b,
        Err(resp) => return Ok(resp),
    };
    let params: Params = sonic_rs::from_slice(&bytes).unwrap_or_default();
    let req = Request::from_parts(parts, bytes);

    let resp = if is_admin {
        admin(state, req, &method_name, params).await
    } else {
        api(state, req, &method_name).await
    };
    Ok(match resp {
        Ok(r) => to_connect(r).await,
        Err(e) => e,
    })
}

fn need<'a>(v: &'a Option<String>, field: &str) -> Result<&'a str, Response<BoxBody>> {
    match v.as_deref() {
        Some(s) if !s.trim().is_empty() => Ok(s),
        _ => Err(err("invalid_argument", &format!("field '{field}' is required"))),
    }
}

fn unimplemented_method(name: &str) -> Response<BoxBody> {
    err("unimplemented", &format!("unknown method '{name}'"))
}

#[allow(unused_variables)]
async fn api(state: Arc<AppState>, req: Request<Bytes>, method: &str) -> Result<Response<BoxBody>, Response<BoxBody>> {
    // 課金アイテム・金融データを扱う入口: `Idempotency-Key` ヘッダ必須(二重防御)。
    crate::middleware::idempotency::check(&req)?;
    Ok(match method {
        "GrantItem" => handlers::items::grant_item(state, req).await,
        "Charge" => handlers::transactions::charge(state, req).await,
        other => return Err(unimplemented_method(other)),
    })
}

#[allow(unused_variables)]
async fn admin(
    state: Arc<AppState>,
    req: Request<Bytes>,
    method: &str,
    p: Params,
) -> Result<Response<BoxBody>, Response<BoxBody>> {
    Ok(match method {
        // リダイレクト
        "UpsertRedirect" => handlers::redirects::upsert_redirect(state, req).await,
        "ListRedirects" => handlers::redirects::list_redirects(state, &req).await,
        "RemoveRedirect" => handlers::redirects::remove_redirect(state, &req, need(&p.host, "host")?).await,
        // 省メモリ/省電力プロファイル
        "GetPowerProfile" => handlers::power_profile::get_power_profile(state, &req).await,
        "SetPowerProfile" => handlers::power_profile::set_power_profile(state, req).await,
        // 自己アップデート
        #[cfg(feature = "auto-update")]
        "GetAutoUpdate" => handlers::self_update::get_auto_update(state, &req).await,
        #[cfg(feature = "auto-update")]
        "SetAutoUpdate" => handlers::self_update::set_auto_update(state, req).await,
        // 死活監視
        "GetWatchdogStatus" => handlers::watchdog::get_watchdog_status(state, &req).await,
        "ListWatchdogExpectations" => handlers::watchdog::list_watchdog_expectations(state, &req).await,
        "SetWatchdogExpectations" => {
            handlers::watchdog::set_watchdog_expectations(state, req, need(&p.host, "host")?).await
        }
        // テナント
        "AddTenant" => handlers::tenants::add_tenant(state, req).await,
        "ListTenants" => handlers::tenants::list_tenants(state, &req).await,
        "UpdateTenant" => handlers::tenants::update_tenant(state, req, need(&p.host, "host")?).await,
        "RemoveTenant" => {
            handlers::tenants::remove_tenant(state, &req, need(&p.host, "host")?, p.path_prefix.clone()).await
        }
        // テナントのTLS
        "UpsertTenantTls" => handlers::tls::upsert_tenant_tls(state, req, need(&p.host, "host")?).await,
        "RemoveTenantTls" => handlers::tls::remove_tenant_tls(state, &req, need(&p.host, "host")?).await,
        #[cfg(feature = "acme")]
        "ObtainTenantTlsViaAcme" => {
            handlers::tls::obtain_tenant_tls_via_acme(state, req, need(&p.host, "host")?).await
        }
        // 静的ファイル/PHP の vhost
        "UpsertWebVhost" => handlers::web_vhost::upsert_web_vhost(state, req).await,
        "ImportWebVhost" => handlers::web_vhost::import_web_vhost(state, req).await,
        "ListWebVhosts" => handlers::web_vhost::list_web_vhosts(state, &req).await,
        "RemoveWebVhost" => handlers::web_vhost::remove_web_vhost(state, &req, need(&p.host, "host")?).await,
        "UpdateWebVhostCompatMode" => {
            handlers::web_vhost::update_compat_mode(state, req, need(&p.host, "host")?).await
        }
        // APIキー(KeyGuardian)
        "IssueKey" => handlers::keys::issue_key(state, req).await,
        "KeyStatus" => handlers::keys::key_status(state, &req).await,
        "RevokeKey" => handlers::keys::revoke_owner(state, req).await,
        // 2FA・ログイン
        #[cfg(feature = "admin-2fa")]
        "Enroll2fa" => handlers::admin_login::enroll(state, req).await,
        #[cfg(feature = "admin-2fa")]
        "Reset2fa" => handlers::admin_login::reset_enrollment(state, req).await,
        #[cfg(feature = "admin-2fa")]
        "Reset2fa" => handlers::admin_login::reset_enrollment(state, req).await,
        #[cfg(feature = "admin-2fa")]
        "Confirm2fa" => handlers::admin_login::confirm(state, req).await,
        #[cfg(feature = "admin-2fa")]
        "Verify2fa" => handlers::admin_login::verify_suspension_override(state, req).await,
        #[cfg(feature = "admin-2fa")]
        "RequestLoginOtp" => handlers::admin_login::request_otp(state, req).await,
        #[cfg(feature = "admin-2fa")]
        "VerifyLogin" => handlers::admin_login::verify_login(state, req).await,
        // ディザスタ・メールバックアップ
        #[cfg(feature = "disaster_email_backup")]
        "SetDisasterEmailBackup" => handlers::disaster_email_backup::set_disaster_email_backup(state, req).await,
        #[cfg(feature = "disaster_email_backup")]
        "VerifyDisasterEmailBackup" => handlers::disaster_email_backup::verify_disaster_email_backup(state, req).await,
        // SFTP 接続情報
        "GetSftpConnectionInfo" => handlers::sftp_info::connection_info(state, &req, p.host.clone()).await,
        // 無料DDNS(DuckDNS)
        "SetupFreeDomain" => handlers::free_domain::setup_free_domain(state, req).await,
        "ListFreeDomains" => handlers::free_domain::list_domains(state, &req).await,
        "RemoveFreeDomain" => handlers::free_domain::remove_domain(state, &req, need(&p.domain, "domain")?).await,
        // 自社ドメインの IPv6 自動更新
        "SetupIpv6AutoUpdate" => handlers::custom_dns_ipv6::setup_ipv6_auto_update(state, req).await,
        "ListIpv6AutoUpdate" => handlers::custom_dns_ipv6::list_ipv6_auto_update(state, &req).await,
        "RemoveIpv6AutoUpdate" => {
            handlers::custom_dns_ipv6::remove_ipv6_auto_update(
                state,
                &req,
                need(&p.base_domain, "base_domain")?,
                need(&p.subdomain, "subdomain")?,
            )
            .await
        }
        // 過去コミット時点の状態の照会(旧 /internal/db/state/...)
        "GetStateAtCommit" => {
            handlers::state_query::get_state_at_commit(
                state,
                &req,
                need(&p.target, "target")?,
                need(&p.key, "key")?,
                need(&p.commit_id, "commit_id")?,
            )
            .await
        }
        other => return Err(unimplemented_method(other)),
    })
}
