//! テナントごとのTLS証明書管理用の内部API(Phase 1: 手動/ACME後段からの
//! 証明書登録)。
//!
//! `open-web-server`自体がSNIに応じて複数ドメインの証明書を切り替えられる
//! ようにする`open_web_server_wire::TenantCertResolver`への薄い配線層。
//! 認証は既存の`handlers::tenants`と同じ共有シークレット方式
//! (`OPEN_WEB_SERVER_ADMIN_TOKEN`)を再利用する。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;

use crate::handlers::tenants::check_admin_auth;
use crate::response::{read_json_body, text_response, BoxBody};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct UpsertTlsCertRequest {
    /// PEM形式の証明書チェーン(leaf + 中間CA、存在すれば)。
    pub cert_pem: String,
    /// PEM形式の秘密鍵。
    pub key_pem: String,
}

/// `POST /admin/tenants/:host/tls` — `host`(SNI名)向けの証明書チェーン+
/// 秘密鍵を登録・更新する。既存の登録は上書き(証明書ローテーション用途)。
/// `tenant_router`にその`host`が登録されているかは意図的にチェックしない
/// ——証明書登録とHTTPルーティング登録は独立した操作であり、TLS終端だけ
/// 先に有効化してからHTTPルーティングを追加する運用(またはその逆)を
/// 妨げないため。
pub async fn upsert_tenant_tls(
    state: Arc<AppState>,
    req: Request<Incoming>,
    host: &str,
) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&req) {
        return resp;
    }

    let body: UpsertTlsCertRequest = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    match state.tls_resolver.upsert_pem(host, body.cert_pem.as_bytes(), body.key_pem.as_bytes()) {
        Ok(()) => text_response(StatusCode::OK, format!("tls certificate registered for '{host}'")),
        Err(e) => text_response(StatusCode::BAD_REQUEST, format!("invalid certificate/key for '{host}': {e}")),
    }
}

/// `DELETE /admin/tenants/:host/tls` — 登録済み証明書を削除する
/// (`tenant_router::remove`と同様、未登録でも冪等に成功する)。
pub async fn remove_tenant_tls(state: Arc<AppState>, req: &Request<Incoming>, host: &str) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(req) {
        return resp;
    }

    match state.tls_resolver.remove(host) {
        Ok(()) => text_response(StatusCode::OK, format!("tls certificate removed for '{host}' (if it existed)")),
        Err(e) => text_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[cfg(feature = "acme")]
#[derive(Deserialize)]
pub struct ObtainAcmeCertRequest {
    /// ACME v2 ディレクトリURL(例: Let's Encrypt本番
    /// "https://acme-v02.api.letsencrypt.org/directory"、ステージング
    /// "https://acme-staging-v02.api.letsencrypt.org/directory")。
    pub directory_url: String,
    pub contact_email: String,
}

/// `POST /admin/tenants/:host/tls/acme` — HTTP-01でACME証明書を実際に
/// 取得し、成功すれば即座に`TenantCertResolver`へ登録する(手動での
/// `POST /admin/tenants/:host/tls`呼び出しが不要になる、自動化された
/// 経路)。`acme` feature有効時のみコンパイルされる。
///
/// **前提条件(呼び出し前に必ず満たすこと)**: HTTP-01検証のため、ACME CAが
/// このプロセスの`GET /.well-known/acme-challenge/*`へ公開インターネット
/// 経由でポート80から到達できる必要がある(本関数はチャレンジの発行・
/// 公開・後始末は自動で行うが、そこへの到達性自体は運用者側の責任)。
#[cfg(feature = "acme")]
pub async fn obtain_tenant_tls_via_acme(
    state: Arc<AppState>,
    req: Request<Incoming>,
    host: &str,
) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&req) {
        return resp;
    }

    let body: ObtainAcmeCertRequest = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    match crate::acme::obtain_certificate_http01(&body.directory_url, host, &body.contact_email, &state.acme_challenges).await {
        Ok((cert_pem, key_pem)) => match state.tls_resolver.upsert_pem(host, cert_pem.as_bytes(), key_pem.as_bytes()) {
            Ok(()) => text_response(StatusCode::OK, format!("acme certificate obtained and registered for '{host}'")),
            Err(e) => text_response(StatusCode::INTERNAL_SERVER_ERROR, format!("acme certificate obtained but registration failed for '{host}': {e}")),
        },
        Err(e) => text_response(StatusCode::BAD_GATEWAY, format!("acme certificate request failed for '{host}': {e}")),
    }
}
