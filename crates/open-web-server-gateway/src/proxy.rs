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
    forward_to_stripped(base_addr, None, req).await
}

/// `forward_to_stripped`と同じだが、転送前にリクエストの`Host`ヘッダを
/// `override_host`が`Some`であればその値へ書き換える(2026-07-24追記、
/// `tenant_router::TenantConfig::override_host`向け——path_prefixタイプの
/// テナント〈例: aruaru.tokyoの`/aruaru/`〉を、転送先バックエンドが
/// 別ホスト名〈例: audiocafe.tokyo〉向けの設定で応答している場合に、
/// バックエンド側へ正しいHostヘッダを送るために使う)。`override_host`が
/// `None`の場合は`forward_to_stripped`と全く同じ挙動(既存呼び出し元への
/// 影響なし)。
pub async fn forward_to_stripped_with_host_override(
    base_addr: &str,
    strip_prefix: Option<&str>,
    override_host: Option<&str>,
    req: Request<Incoming>,
) -> Response<BoxBody> {
    let req = match override_host {
        Some(host) => match hyper::header::HeaderValue::from_str(host) {
            Ok(value) => {
                let (mut parts, body) = req.into_parts();
                parts.headers.insert(hyper::header::HOST, value);
                Request::from_parts(parts, body)
            }
            Err(e) => {
                return text_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("invalid override_host '{host}': {e}"),
                )
            }
        },
        None => req,
    };

    forward_to_stripped(base_addr, strip_prefix, req).await
}

/// `forward_to`と同じだが、転送前に`strip_prefix`(例: `"/blog"`)を
/// リクエストパスの先頭から除去してから転送する(2026-07-22追記、
/// `tenant_router::TenantConfig::path_prefix`向け——RS-Blog/RS-Chiketto/
/// RS-EC等のバックエンドはいずれも`/`をトップとして期待するルーティング
/// 実装のため、プレフィックス込みのパスをそのまま渡すと404になる)。
/// `strip_prefix`が`None`または一致しない場合は`forward_to`と全く同じ
/// 挙動(既存呼び出し元への影響なし)。
pub async fn forward_to_stripped(
    base_addr: &str,
    strip_prefix: Option<&str>,
    req: Request<Incoming>,
) -> Response<BoxBody> {
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

    let original_path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    let stripped_path_and_query;
    let path_and_query = match strip_prefix {
        Some(prefix) if !prefix.is_empty() => {
            let path = parts.uri.path();
            let query = parts.uri.query();
            let new_path = crate::tenant_router::strip_path_prefix(path, prefix);
            stripped_path_and_query = match query {
                Some(q) => format!("{new_path}?{q}"),
                None => new_path,
            };
            stripped_path_and_query.as_str()
        }
        _ => original_path_and_query,
    };

    let upstream_uri: Uri = match format!("{base_url}{path_and_query}").parse() {
        Ok(uri) => uri,
        Err(e) => {
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("invalid upstream URI for '{base_url}': {e}"),
            )
        }
    };

    let build_upstream_req = || -> Result<Request<Full<Bytes>>, String> {
        let mut b = Request::builder()
            .method(parts.method.clone())
            .uri(upstream_uri.clone());
        for (name, value) in parts.headers.iter() {
            b = b.header(name, value);
        }
        b.body(Full::new(body_bytes.clone())).map_err(|e| e.to_string())
    };

    let upstream_req = match build_upstream_req() {
        Ok(req) => req,
        Err(e) => {
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to build upstream request: {e}"),
            )
        }
    };

    let attempt = request_with_one_retry_on_connect_failure(upstream_req, &build_upstream_req).await;

    match attempt {
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

/// 接続確立自体の失敗(connection refused/reset/EOF等、`hyper_util`の
/// `Error::is_connect()`が真になるクラス)のみを対象に、短い待機を
/// 挟んで1回だけ再送する。**到達してエラー応答(4xx/5xx)が返った
/// ケースは対象外**(サーバーに到達した以上、再送は二重実行の
/// リスクを生むため)。2026-08-04、open-web-server↔RPoem実接続検証で
/// 「動的登録された直後のバックエンドが一瞬だけ接続を受け付けない」
/// 実際のレースを観測したことを受けて追加。
async fn request_with_one_retry_on_connect_failure(
    first_req: Request<Full<Bytes>>,
    rebuild: &(dyn Fn() -> Result<Request<Full<Bytes>>, String> + Send + Sync),
) -> Result<
    Response<Incoming>,
    hyper_util::client::legacy::Error,
> {
    let first = shared_client().request(first_req).await;
    match first {
        Err(e) if is_transient_connection_failure(&e) => {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            match rebuild() {
                Ok(retry_req) => shared_client().request(retry_req).await,
                Err(_) => Err(e),
            }
        }
        other => other,
    }
}

/// リクエストが実際にupstreamへ到達し処理された(エラー応答を含む)
/// のではなく、「接続自体が確立できなかった/確立直後に切断された」
/// ことを示すエラーかどうかを判定する。`hyper_util::Error::is_connect()`
/// は`ErrorKind::Connect`(TCP接続自体の失敗)のみを対象とするため、
/// 「acceptはしたが処理前に切断された」ケース(2026-08-04の実E2E検証で
/// 実際に観測、RPoem側`ThreadedProxyServer`のワーカー起動直後の
/// レース)は`ErrorKind::SendRequest`/`Canceled`として現れ捕捉されない
/// ——`hyper::Error`(`source()`経由)の`is_canceled()`/`is_closed()`/
/// `is_incomplete_message()`まで見て判定を広げる。
fn is_transient_connection_failure(e: &hyper_util::client::legacy::Error) -> bool {
    if e.is_connect() {
        return true;
    }
    if let Some(source) = std::error::Error::source(e) {
        if let Some(hyper_err) = source.downcast_ref::<hyper::Error>() {
            return hyper_err.is_canceled()
                || hyper_err.is_closed()
                || hyper_err.is_incomplete_message();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener as StdTcpListener;

    fn build(uri: &str) -> Request<Full<Bytes>> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .body(Full::new(Bytes::new()))
            .unwrap()
    }

    /// 実際にE2E検証で観測された「バックエンド起動直後の一瞬だけ接続が
    /// 失敗する」状況を再現する: 1回目の接続は即座に切断(応答を返さず
    /// 閉じる)、2回目は正しく応答する。`request_with_one_retry_on_
    /// connect_failure`が自動的に1回リトライして最終的に200を返すことを
    /// 実TCP接続で検証する。
    #[tokio::test]
    async fn retries_once_and_recovers_from_a_transient_connect_failure() {
        let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let tokio_listener = tokio::net::TcpListener::from_std(listener).unwrap();

        tokio::spawn(async move {
            // 1回目: 接続だけ受けて即座に閉じる(応答なし)。
            let (stream, _) = tokio_listener.accept().await.unwrap();
            drop(stream);

            // 2回目: 正常に200を返す。
            let (mut stream, _) = tokio_listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let body = "recovered";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(resp.as_bytes()).await.unwrap();
        });

        let uri = format!("http://{addr}/");
        let req = build(&uri);
        let uri_for_retry = uri.clone();
        let result = request_with_one_retry_on_connect_failure(req, &|| Ok(build(&uri_for_retry))).await;

        let resp = result.expect("should recover on retry");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// 到達後にエラー応答(サーバーがリクエストを処理し、その上で
    /// 明示的にエラーを返した)は再送しないことを確認する
    /// (=このリトライは「到達すらしなかった」場合限定であることの実証)。
    #[tokio::test]
    async fn does_not_retry_after_reaching_the_server_even_on_error_status() {
        let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let tokio_listener = tokio::net::TcpListener::from_std(listener).unwrap();

        let accept_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let accept_count_bg = accept_count.clone();
        tokio::spawn(async move {
            let (mut stream, _) = tokio_listener.accept().await.unwrap();
            accept_count_bg.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await;
            let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            stream.write_all(resp.as_bytes()).await.unwrap();
        });

        let uri = format!("http://{addr}/");
        let req = build(&uri);
        let uri_for_retry = uri.clone();
        let result = request_with_one_retry_on_connect_failure(req, &|| Ok(build(&uri_for_retry))).await;

        let resp = result.expect("server responded, no connect error");
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        // わずかに待って、再送が発生していないこと(accept呼び出しが1回のみ)を確認。
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(accept_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
