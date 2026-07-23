//! CORS (Cross-Origin Resource Sharing) 対応。
//!
//! `open-easy-web`(ブラウザ上のWASM)のドメイン設定ウィザードが、
//! `open-web-server`の管理API(`/admin/*`)を別オリジン(別ポート/別ホスト)
//! から`fetch()`で叩けるようにするためのオプトイン機能。
//!
//! **既定で無効**(`OPEN_WEB_SERVER_CORS_ALLOWED_ORIGINS`環境変数が
//! 未設定の場合、CORSヘッダーは一切付与しない=既存動作を完全に維持する)。
//! 有効化するには、許可するオリジンをカンマ区切りで設定する:
//!
//! ```text
//! OPEN_WEB_SERVER_CORS_ALLOWED_ORIGINS=http://localhost:8080,https://example.com
//! ```
//!
//! `app_proxy`/`free_domain`と同じ作法(環境変数を都度読む薄い関数群、
//! プロセス全体で共有するキャッシュ状態は持たない)に揃えている。

use hyper::header::{HeaderMap, HeaderName, HeaderValue};
use hyper::{Method, Response, StatusCode};

use crate::response::BoxBody;

const CORS_ALLOWED_ORIGINS_ENV: &str = "OPEN_WEB_SERVER_CORS_ALLOWED_ORIGINS";

/// 管理API(`x-admin-token`)・`Idempotency-Key`・標準的な`Content-Type`
/// ヘッダーを許可する。ウィザード側がJSONボディを送る `POST`/`PUT`/`DELETE`
/// と、一覧取得の `GET` を想定し、`OPTIONS` はプリフライト自体のために含む。
const ALLOWED_METHODS: &str = "GET, POST, PUT, DELETE, OPTIONS";
const ALLOWED_HEADERS: &str = "content-type, x-admin-token, authorization, idempotency-key";

/// このプロセスでCORSが有効化されているか(=環境変数が設定されているか)。
#[cfg(test)]
pub fn is_enabled() -> bool {
    allowed_origins().is_some()
}

/// 設定された許可オリジンのリストを返す。未設定なら `None`
/// (CORS機能自体が無効、既存動作を完全に維持する)。
fn allowed_origins() -> Option<Vec<String>> {
    let raw = std::env::var(CORS_ALLOWED_ORIGINS_ENV).ok()?;
    let origins: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if origins.is_empty() {
        None
    } else {
        Some(origins)
    }
}

/// リクエストの`Origin`ヘッダーが許可リストに含まれているかを判定する。
/// CORS自体が無効な場合、または`Origin`ヘッダーが無い場合は `None`。
fn matched_origin(headers: &HeaderMap) -> Option<String> {
    let origins = allowed_origins()?;
    let origin = headers.get(hyper::header::ORIGIN)?.to_str().ok()?;
    origins.iter().find(|o| o.as_str() == origin).cloned()
}

/// `OPTIONS`プリフライトリクエストかどうかを判定する
/// (`Access-Control-Request-Method`ヘッダーの有無で判定、ブラウザが
/// 実際に送るプリフライトの形に合わせる——単なる`OPTIONS`メソッドの
/// リクエスト全てをプリフライト扱いにはしない)。
fn is_preflight(method: &Method, headers: &HeaderMap) -> bool {
    method == Method::OPTIONS && headers.contains_key("access-control-request-method")
}

/// プリフライトリクエストであれば、許可オリジンからのものに限り
/// `204 No Content` + CORSヘッダーの即時応答を返す。プリフライト対象外
/// (通常リクエスト、CORS無効、許可されていないオリジン)なら `None`
/// (=呼び出し側は通常の`dispatch`へ進む)。
pub fn handle_preflight(method: &Method, headers: &HeaderMap) -> Option<Response<BoxBody>> {
    if !is_preflight(method, headers) {
        return None;
    }
    let origin = matched_origin(headers)?;

    let mut builder = Response::builder().status(StatusCode::NO_CONTENT);
    builder = apply_headers_to_builder(builder, &origin);
    Some(
        builder
            .body(http_body_util::Full::new(bytes::Bytes::new()))
            .expect("preflight response is always well-formed"),
    )
}

/// 通常リクエストの応答へ、リクエストの`Origin`が許可リストに含まれる
/// 場合のみCORSヘッダーを追加する。許可されていないオリジン・CORS無効・
/// `Origin`ヘッダー無しのいずれの場合もレスポンスを変更しない
/// (=許可されていないオリジンにはCORSヘッダーを一切付けない)。
pub fn apply_response_headers(request_headers: &HeaderMap, mut response: Response<BoxBody>) -> Response<BoxBody> {
    let Some(origin) = matched_origin(request_headers) else {
        return response;
    };

    let headers = response.headers_mut();
    insert_header(headers, "access-control-allow-origin", &origin);
    insert_header(headers, "access-control-allow-methods", ALLOWED_METHODS);
    insert_header(headers, "access-control-allow-headers", ALLOWED_HEADERS);
    insert_header(headers, "vary", "origin");
    response
}

fn apply_headers_to_builder(mut builder: http::response::Builder, origin: &str) -> http::response::Builder {
    if let Some(headers) = builder.headers_mut() {
        insert_header(headers, "access-control-allow-origin", origin);
        insert_header(headers, "access-control-allow-methods", ALLOWED_METHODS);
        insert_header(headers, "access-control-allow-headers", ALLOWED_HEADERS);
        insert_header(headers, "vary", "origin");
    }
    builder
}

fn insert_header(headers: &mut hyper::HeaderMap, name: &'static str, value: &str) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(name.as_bytes()),
        HeaderValue::from_str(value),
    ) {
        headers.insert(name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // `std::env::set_var`はプロセス全体のグローバル状態であり、この
    // モジュールのテストを`cargo test`の既定の並列実行に任せると
    // 互いの環境変数書き換えが競合するため、`main.rs`の
    // `ADMIN_TOKEN_ENV_LOCK`と同じパターンで直列化する。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<F: FnOnce()>(value: Option<&str>, f: F) {
        let _guard = ENV_LOCK.lock().unwrap();
        match value {
            Some(v) => std::env::set_var(CORS_ALLOWED_ORIGINS_ENV, v),
            None => std::env::remove_var(CORS_ALLOWED_ORIGINS_ENV),
        }
        f();
        std::env::remove_var(CORS_ALLOWED_ORIGINS_ENV);
    }

    fn headers_with_origin(origin: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(hyper::header::ORIGIN, HeaderValue::from_str(origin).unwrap());
        h
    }

    #[test]
    fn disabled_by_default_when_env_unset() {
        with_env(None, || {
            assert!(!is_enabled());
            let headers = headers_with_origin("http://localhost:9000");
            assert!(matched_origin(&headers).is_none());
        });
    }

    #[test]
    fn matches_allowed_origin_from_comma_separated_list() {
        with_env(Some("http://a.example, http://b.example"), || {
            let headers = headers_with_origin("http://b.example");
            assert_eq!(matched_origin(&headers), Some("http://b.example".to_string()));
        });
    }

    #[test]
    fn rejects_origin_not_in_allow_list() {
        with_env(Some("http://a.example"), || {
            let headers = headers_with_origin("http://evil.example");
            assert!(matched_origin(&headers).is_none());
        });
    }

    #[test]
    fn preflight_detection_requires_request_method_header() {
        let plain_options_headers = HeaderMap::new();
        assert!(!is_preflight(&Method::OPTIONS, &plain_options_headers));

        let mut real_preflight_headers = HeaderMap::new();
        real_preflight_headers.insert("access-control-request-method", HeaderValue::from_static("POST"));
        assert!(is_preflight(&Method::OPTIONS, &real_preflight_headers));
    }

    #[test]
    fn handle_preflight_returns_none_when_disabled() {
        with_env(None, || {
            let mut headers = headers_with_origin("http://a.example");
            headers.insert("access-control-request-method", HeaderValue::from_static("POST"));
            assert!(handle_preflight(&Method::OPTIONS, &headers).is_none());
        });
    }

    #[test]
    fn handle_preflight_returns_204_with_headers_for_allowed_origin() {
        with_env(Some("http://a.example"), || {
            let mut headers = headers_with_origin("http://a.example");
            headers.insert("access-control-request-method", HeaderValue::from_static("POST"));
            headers.insert(
                "access-control-request-headers",
                HeaderValue::from_static("x-admin-token"),
            );
            let resp = handle_preflight(&Method::OPTIONS, &headers)
                .expect("allowed origin should get a preflight response");
            assert_eq!(resp.status(), StatusCode::NO_CONTENT);
            assert_eq!(
                resp.headers().get("access-control-allow-origin").unwrap(),
                "http://a.example"
            );
            assert!(resp
                .headers()
                .get("access-control-allow-headers")
                .unwrap()
                .to_str()
                .unwrap()
                .contains("x-admin-token"));
        });
    }

    #[test]
    fn handle_preflight_returns_none_for_disallowed_origin() {
        with_env(Some("http://a.example"), || {
            let mut headers = headers_with_origin("http://evil.example");
            headers.insert("access-control-request-method", HeaderValue::from_static("POST"));
            assert!(handle_preflight(&Method::OPTIONS, &headers).is_none());
        });
    }

    #[test]
    fn apply_response_headers_adds_headers_only_for_allowed_origin() {
        with_env(Some("http://a.example"), || {
            let allowed_headers = headers_with_origin("http://a.example");
            let resp = crate::response::text_response(StatusCode::OK, "ok");
            let resp = apply_response_headers(&allowed_headers, resp);
            assert_eq!(
                resp.headers().get("access-control-allow-origin").unwrap(),
                "http://a.example"
            );

            let disallowed_headers = headers_with_origin("http://evil.example");
            let resp2 = crate::response::text_response(StatusCode::OK, "ok");
            let resp2 = apply_response_headers(&disallowed_headers, resp2);
            assert!(resp2.headers().get("access-control-allow-origin").is_none());
        });
    }
}
