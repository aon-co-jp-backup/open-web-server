//! テナントのバックエンド(open-runo / poem-cosmo-tauri)へのHTTPリバース
//! プロキシ転送。
//!
//! `tenant_router::TenantRegistry::resolve()` で解決した `TenantHandle` の
//! `backend_addr` へ、受信した `Request` をほぼそのまま中継する。
//! 接続プールは `hyper_util::client::legacy::Client` が内部で
//! キープアライブ管理するため、ドメインごとに新規プロセス/新規接続を
//! 都度張り直すことはない(「分身の術」= 接続もタスク単位で使い回す)。

use std::sync::OnceLock;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode, Uri};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

use crate::response::{text_response, BoxBody};

type ProxyClient = Client<HttpConnector, Full<Bytes>>;

/// プロセス全体で1つの `Client` を使い回す(ドメイン数・委譲先が増えても
/// クライアント自体は増やさない。内部のコネクションプールがホストごとに
/// キープアライブ接続を管理する)。`tenant_router`経由のマルチテナント転送
/// と`app_proxy`経由の単一アップストリーム転送の両方がこれを共有する。
fn shared_client() -> &'static ProxyClient {
    static CLIENT: OnceLock<ProxyClient> = OnceLock::new();
    CLIENT.get_or_init(|| Client::builder(TokioExecutor::new()).build(HttpConnector::new()))
}

/// 受信リクエストを `base_addr` (例: `"127.0.0.1:9001"` または
/// `"http://127.0.0.1:8080"`)へそのまま転送し、応答を返す。
///
/// `base_addr` に scheme が無ければ `http://` を補う(`tenant_router`の
/// `backend_addr`はホスト:ポートのみを想定している一方、`app_proxy`の
/// `OPEN_WEB_SERVER_APP_UPSTREAM`は完全なURLを想定しているため、両方を
/// 同じ関数で受けられるようにする)。
///
/// 転送失敗(バックエンド接続不可等)は `502 Bad Gateway` にマッピングする。
pub async fn forward_to(base_addr: &str, req: Request<Incoming>) -> Response<BoxBody> {
    let base_addr = base_addr.trim_end_matches('/');
    let base_url = if base_addr.contains("://") {
        base_addr.to_string()
    } else {
        format!("http://{base_addr}")
    };

    let (parts, body) = req.into_parts();

    let body_bytes = match BodyExt::collect(body).await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            return text_response(
                StatusCode::BAD_REQUEST,
                format!("failed to read request body: {e}"),
            )
        }
    };

    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    let upstream_uri: Uri = match format!("{base_url}{path_and_query}").parse() {
        Ok(uri) => uri,
        Err(e) => {
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("invalid upstream URI for '{base_url}': {e}"),
            )
        }
    };

    let mut upstream_req = Request::builder()
        .method(parts.method.clone())
        .uri(upstream_uri);

    for (name, value) in parts.headers.iter() {
        upstream_req = upstream_req.header(name, value);
    }

    let upstream_req = match upstream_req.body(Full::new(body_bytes)) {
        Ok(req) => req,
        Err(e) => {
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to build upstream request: {e}"),
            )
        }
    };

    match shared_client().request(upstream_req).await {
        Ok(upstream_resp) => {
            let (parts, body) = upstream_resp.into_parts();
            match BodyExt::collect(body).await {
                Ok(collected) => Response::from_parts(parts, Full::new(collected.to_bytes())),
                Err(e) => text_response(
                    StatusCode::BAD_GATEWAY,
                    format!("failed to read upstream response body: {e}"),
                ),
            }
        }
        Err(e) => text_response(
            StatusCode::BAD_GATEWAY,
            format!("failed to reach upstream '{base_url}': {e}"),
        ),
    }
}
