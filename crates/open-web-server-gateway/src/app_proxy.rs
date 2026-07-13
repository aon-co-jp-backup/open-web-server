//! Apache↔Tomcat型の「Webサーバー / アプリケーションサーバー」連携。
//!
//! `open-web-server-gateway` はこのモジュール無しでも完全に単体動作する
//! (課金/決済ハンドラは常に自前で処理する)。`OPEN_WEB_SERVER_APP_UPSTREAM`
//! 環境変数が設定されている場合に限り、既存ハンドラのどのパスにも一致
//! しなかったリクエストを、より高速な動的処理を担うアプリケーション
//! サーバー層(`open-runo` または `poem-cosmo-tauri` の
//! `open-runo-router`、既定では `0.0.0.0:8080` で待受)へ転送する。
//!
//! Apache が静的配信+`mod_proxy_ajp`でTomcatへ動的処理を委譲し、Tomcat
//! 単体でも直接HTTPを受けられるのと同じ関係——ここではAJPではなく単純な
//! HTTPリバースプロキシで代替する(既存の`open-easyweb`
//! `gen-vhost.sh --stack=proxy`がnginx/Apache→本ゲートウェイ間で使うのと
//! 同じ形式に揃えた)。

use std::env;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

use crate::response::{text_response, BoxBody};

const APP_UPSTREAM_ENV: &str = "OPEN_WEB_SERVER_APP_UPSTREAM";

/// アプリケーションサーバー層への転送先URL(例: `http://127.0.0.1:8080`)。
/// 環境変数が未設定なら `None`(=単体動作、このモジュールは一切使われない)。
pub fn app_upstream_base() -> Option<String> {
    env::var(APP_UPSTREAM_ENV)
        .ok()
        .map(|v| v.trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
}

/// リクエストをアプリケーションサーバー層へそのまま転送し、応答を
/// そのまま呼び出し元へ返す(パス・クエリ・メソッド・ボディを保持)。
///
/// アプリケーションサーバーに到達できない場合は `502 Bad Gateway` を返す
/// (Apache/Tomcat連携でTomcatが落ちている場合の挙動と同等——本ゲートウェイ
/// 自身の課金/決済ハンドラには一切影響しない)。
pub async fn forward(base_url: &str, req: Request<Incoming>) -> Response<BoxBody> {
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let target = format!("{base_url}{path_and_query}");

    let uri: hyper::Uri = match target.parse() {
        Ok(u) => u,
        Err(e) => {
            return text_response(
                StatusCode::BAD_GATEWAY,
                format!("invalid app-server upstream URL '{target}': {e}"),
            )
        }
    };

    let (parts, body) = req.into_parts();
    let body_bytes = match BodyExt::collect(body).await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            return text_response(
                StatusCode::BAD_GATEWAY,
                format!("failed to read request body for app-server forward: {e}"),
            )
        }
    };

    let mut builder = Request::builder().method(parts.method.clone()).uri(uri);
    for (name, value) in parts.headers.iter() {
        builder = builder.header(name, value);
    }
    let forwarded_req = match builder.body(Full::new(body_bytes)) {
        Ok(r) => r,
        Err(e) => {
            return text_response(
                StatusCode::BAD_GATEWAY,
                format!("failed to build forwarded request: {e}"),
            )
        }
    };

    let client: Client<HttpConnector, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build(HttpConnector::new());

    match client.request(forwarded_req).await {
        Ok(resp) => {
            let (parts, body) = resp.into_parts();
            let bytes = match BodyExt::collect(body).await {
                Ok(collected) => collected.to_bytes(),
                Err(e) => {
                    return text_response(
                        StatusCode::BAD_GATEWAY,
                        format!("failed to read app-server response body: {e}"),
                    )
                }
            };
            Response::from_parts(parts, Full::new(bytes))
        }
        Err(e) => text_response(
            StatusCode::BAD_GATEWAY,
            format!("app-server upstream '{base_url}' unreachable: {e}"),
        ),
    }
}
