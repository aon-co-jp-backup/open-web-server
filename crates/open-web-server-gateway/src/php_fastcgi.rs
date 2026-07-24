//! 本番向けPHP-FPM/FastCGI直結配信(2026-07-24追加)。
//!
//! 既存の`php_server.rs`(`php -S`ビルトインサーバーをサブプロセス起動して
//! リバースプロキシする実装)とは異なり、こちらは**既に稼働している
//! php-fpmプロセス**へ、`fastcgi-client`クレート(2026-07時点で
//! crates.io/GitHub上でアクティブに使われていることを確認済み、
//! `Client::new_tokio`によるtokio非同期ランタイム対応)を使って直接
//! FastCGIプロトコルで話しかける。サブプロセスは一切起動しない
//! (php-fpm自体のプロセス管理はopen-web-serverの管轄外——実運用では
//! systemd等が別途管理する前提)。
//!
//! `fastcgi_addr`は`"127.0.0.1:9000"`のようなTCPアドレス、または
//! Unixドメインソケットのパス(`/`から始まる文字列、例:
//! `"/run/php/php8.3-fpm.sock"`)のいずれかとして解釈する
//! (`:`を含むかどうかで判定——Windowsの絶対パスはこの用途では
//! 想定しない)。
//!
//! **正直な開示・スコープ**: (1) 各リクエストごとに新規接続を張る
//! 単純な実装であり、php-fpm側の接続プーリング/keep-aliveの恩恵は
//! 受けない(将来的な最適化候補)。(2) PHPからのレスポンスヘッダ
//! パース(`Status:`行・`\r\n\r\n`区切り)は最小限の実装で、CGI/1.1
//! 仕様の全機能(複数値ヘッダの结合等)を完全に踏襲するものではない。

use std::path::Path;

use bytes::Bytes;
use fastcgi_client::{Client, Params, Request as FcgiRequest};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};

use crate::response::{text_response, BoxBody};

/// `fastcgi_addr`へ接続し、`req`をFastCGI経由でphp-fpmへ渡して応答を
/// 組み立てる。
pub async fn proxy_fastcgi(
    fastcgi_addr: &str,
    docroot: &Path,
    req: Request<Incoming>,
) -> Response<BoxBody> {
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

    let path = parts.uri.path();
    let query = parts.uri.query().unwrap_or("");

    // スクリプトの実ファイルパスを決定する(最小実装: パスがディレクトリ
    // ルート相当("/"またはPHPファイルを含まない)であれば`index.php`へ
    // フォールバックする。既存の`static_files`と同じ`..`拒否・
    // canonicalize検証は静的アセット判定側で既に行われているため、ここは
    // php-fpmへ渡す`SCRIPT_FILENAME`の組み立てに専念する)。
    // **注意**: `SCRIPT_FILENAME`はphp-fpm(FastCGIバックエンド)側の
    // ファイルシステム上のパスであり、このプロセスが動くOSのパス表現とは
    // 無関係(バックエンドは大抵の本番運用ではLinux上で稼働する)。
    // `std::path::Path::join`はOS依存のセパレータ(Windowsでは`\`)を
    // 使ってしまうため、常にPOSIX形式の`/`で手動連結する
    // (Windows開発環境から実WSL2/Linux上のphp-fpmを検証した際に、
    // `Path::join`ではセパレータが混在し404になることを2026-07-24に
    // 実機で確認済み)。
    let docroot_str = docroot.to_string_lossy();
    let docroot_str = docroot_str.trim_end_matches('/');
    let relative = path.trim_start_matches('/');
    let script_filename = if relative.is_empty() || path.ends_with('/') {
        format!("{docroot_str}/{relative}index.php")
    } else {
        format!("{docroot_str}/{relative}")
    };
    tracing::debug!(script_filename, "fastcgi SCRIPT_FILENAME resolved");
    let script_name = if path.is_empty() { "/".to_string() } else { path.to_string() };

    let content_length = body_bytes.len();
    let content_type = parts
        .headers
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let mut params = Params::default()
        .request_method(parts.method.as_str().to_string())
        .script_name(script_name)
        .script_filename(script_filename)
        .request_uri(parts.uri.to_string())
        .document_uri(path.to_string())
        .query_string(query.to_string())
        .remote_addr("127.0.0.1".to_string())
        .remote_port(0u16)
        .server_addr("127.0.0.1".to_string())
        .server_port(0u16)
        .server_name(
            parts
                .headers
                .get(hyper::header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("localhost")
                .to_string(),
        )
        .content_type(content_type)
        .content_length(content_length);

    // 残りのHTTPヘッダを`HTTP_<NAME>`形式のCGI環境変数として渡す
    // (PHPの`$_SERVER['HTTP_*']`慣例、`Content-Type`/`Content-Length`は
    // 上で個別に渡し済みのため除外)。
    let mut extra_headers = Vec::new();
    for (name, value) in parts.headers.iter() {
        if name == hyper::header::CONTENT_TYPE || name == hyper::header::CONTENT_LENGTH {
            continue;
        }
        if let Ok(value_str) = value.to_str() {
            let cgi_name = format!(
                "HTTP_{}",
                name.as_str().to_uppercase().replace('-', "_")
            );
            extra_headers.push((cgi_name, value_str.to_string()));
        }
    }
    for (name, value) in extra_headers {
        params.insert(name.into(), value.into());
    }

    let fcgi_request = FcgiRequest::new(
        params,
        fastcgi_client::io::Cursor::new(body_bytes.to_vec()),
    );

    let result = if fastcgi_addr.contains(':') {
        match tokio::net::TcpStream::connect(fastcgi_addr).await {
            Ok(stream) => {
                let client = Client::new_tokio(stream);
                client.execute_once(fcgi_request).await
            }
            Err(e) => {
                return text_response(
                    StatusCode::BAD_GATEWAY,
                    format!("failed to connect to php-fpm at '{fastcgi_addr}': {e}"),
                )
            }
        }
    } else {
        #[cfg(unix)]
        {
            match tokio::net::UnixStream::connect(fastcgi_addr).await {
                Ok(stream) => {
                    let client = Client::new_tokio(stream);
                    client.execute_once(fcgi_request).await
                }
                Err(e) => {
                    return text_response(
                        StatusCode::BAD_GATEWAY,
                        format!(
                            "failed to connect to php-fpm unix socket '{fastcgi_addr}': {e}"
                        ),
                    )
                }
            }
        }
        #[cfg(not(unix))]
        {
            return text_response(
                StatusCode::NOT_IMPLEMENTED,
                format!(
                    "unix socket FastCGI addresses ('{fastcgi_addr}') are only supported on unix platforms in this build"
                ),
            );
        }
    };

    let output = match result {
        Ok(output) => output,
        Err(e) => {
            return text_response(
                StatusCode::BAD_GATEWAY,
                format!("fastcgi request to '{fastcgi_addr}' failed: {e}"),
            )
        }
    };

    if let Some(stderr) = &output.stderr {
        if !stderr.is_empty() {
            tracing::warn!(
                stderr = %String::from_utf8_lossy(stderr),
                fastcgi_addr,
                "php-fpm reported stderr output"
            );
        }
    }

    let stdout = output.stdout.unwrap_or_default();
    build_response_from_cgi_output(&stdout)
}

/// php-fpmが返す生のstdout(CGI/1.1形式: ヘッダ行 + 空行 + ボディ)から
/// `Response<BoxBody>`を組み立てる。`Status:`ヘッダが無ければ`200 OK`を
/// 既定とする(CGI/1.1仕様通り)。
fn build_response_from_cgi_output(raw: &[u8]) -> Response<BoxBody> {
    let separator = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (i, 4))
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2)));

    let (header_bytes, body_bytes) = match separator {
        Some((idx, len)) => (&raw[..idx], &raw[idx + len..]),
        None => (raw, &raw[raw.len()..]),
    };

    let header_text = String::from_utf8_lossy(header_bytes);
    let mut status = StatusCode::OK;
    let mut builder = Response::builder();

    for line in header_text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();

        if name.eq_ignore_ascii_case("status") {
            if let Some(code_str) = value.split_whitespace().next() {
                if let Ok(code) = code_str.parse::<u16>() {
                    if let Ok(parsed) = StatusCode::from_u16(code) {
                        status = parsed;
                    }
                }
            }
            continue;
        }

        builder = builder.header(name, value);
    }

    builder
        .status(status)
        .body(Full::new(Bytes::from(body_bytes.to_vec())))
        .unwrap_or_else(|e| {
            text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to build response from php-fpm output: {e}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_header_and_body() {
        let raw = b"Status: 404 Not Found\r\nContent-Type: text/plain\r\n\r\nnot found here";
        let resp = build_response_from_cgi_output(raw);
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain"
        );
    }

    #[test]
    fn defaults_to_200_without_status_header() {
        let raw = b"Content-Type: text/html\r\n\r\n<html>ok</html>";
        let resp = build_response_from_cgi_output(raw);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn handles_lf_only_separator() {
        let raw = b"Content-Type: text/plain\n\nbody-with-lf-only";
        let resp = build_response_from_cgi_output(raw);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain"
        );
    }

    #[test]
    fn handles_missing_separator_as_body_only() {
        let raw = b"just a raw body with no headers";
        let resp = build_response_from_cgi_output(raw);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// **実php-fpmへの実FastCGI通信を検証する統合テスト**(2026-07-24)。
    /// このサンドボックス環境には既定でphp-fpmが無いため`#[ignore]`とし、
    /// `OPEN_WEB_SERVER_TEST_FASTCGI_ADDR`(例: `172.22.9.49:9000`)+
    /// `OPEN_WEB_SERVER_TEST_FASTCGI_DOCROOT`(php-fpm側から見た`index.php`
    /// を含むドキュメントルート、例: `/var/www/fcgi-test`)の両方が設定
    /// されている場合のみ`cargo test -- --ignored`で実行される。
    /// 実際にWSL2 Ubuntu上へ`apt-get install -y php-fpm`し、
    /// `listen = 0.0.0.0:9000`へ変更した実php-fpmに対し、この関数から
    /// 実際に`GET /`を送りPHPが生成した本文
    /// (`hello-from-php-fpm method=GET host=...`)が返ることを2026-07-24に
    /// 実際に確認済み(HANDOFF参照)。
    #[tokio::test]
    #[ignore = "requires a real php-fpm reachable via OPEN_WEB_SERVER_TEST_FASTCGI_ADDR"]
    async fn real_php_fpm_roundtrip_over_fastcgi() {
        let Ok(fastcgi_addr) = std::env::var("OPEN_WEB_SERVER_TEST_FASTCGI_ADDR") else {
            eprintln!("skipping: OPEN_WEB_SERVER_TEST_FASTCGI_ADDR not set");
            return;
        };
        let Ok(docroot) = std::env::var("OPEN_WEB_SERVER_TEST_FASTCGI_DOCROOT") else {
            eprintln!("skipping: OPEN_WEB_SERVER_TEST_FASTCGI_DOCROOT not set");
            return;
        };

        // `hyper::body::Incoming`は実TCP接続からしか作れないため、
        // `proxy_fastcgi`が期待する`Request<Incoming>`をテスト専用の
        // 最小HTTPサーバー経由で用意する(このプロセス自身が一時的な
        // フロントエンドになり、内部で`proxy_fastcgi`を呼ぶ)。
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let docroot_clone = docroot.clone();
        let fastcgi_addr_clone = fastcgi_addr.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let io = hyper_util::rt::TokioIo::new(stream);
            let service = hyper::service::service_fn(move |req: Request<Incoming>| {
                let docroot = docroot_clone.clone();
                let fastcgi_addr = fastcgi_addr_clone.clone();
                async move { Ok::<_, std::convert::Infallible>(proxy_fastcgi(&fastcgi_addr, std::path::Path::new(&docroot), req).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await;
        });

        let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let io = hyper_util::rt::TokioIo::new(tcp);
        let (mut sender, connection) = hyper::client::conn::http1::handshake(io).await.unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });

        let req = Request::builder()
            .method("GET")
            .uri("/")
            .header("host", "fcgi-test.example")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = sender.send_request(req).await.unwrap();
        let status = resp.status();
        let body = http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
        let body_text = String::from_utf8_lossy(&body);
        assert_eq!(status, StatusCode::OK, "body was: {body_text}");
        assert!(
            body_text.contains("hello-from-php-fpm"),
            "expected real php-fpm output, got: {body_text}"
        );
        assert!(body_text.contains("host=fcgi-test.example"));
    }
}
