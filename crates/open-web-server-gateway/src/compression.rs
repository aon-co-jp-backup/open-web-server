//! Nginx互換機能の1つ: レスポンスのgzip圧縮(2026-07-23新設)。
//!
//! `open-web-server`は「Apache+Nginxハイブリッド配信エンジン」を目標に
//! 掲げているが、監査の結果gzip/brotli圧縮機能が一切実装されていない
//! ことが判明した(RPoem側`open-runo-router::middleware_hyper::
//! with_compression`には既に実装済み・実績あり——本モジュールは
//! そのロジックを、この`open-web-server-gateway`の`Response<BoxBody>`
//! (`BoxBody = Full<Bytes>`、ボディが既にメモリ上に全て揃っている)
//! という型に合わせて移植したもの)。
//!
//! **スコープ**: gzipのみ(brotliは未実装、次回以降の課題)。

use bytes::Bytes;
use flate2::write::GzEncoder;
use flate2::Compression;
use http_body_util::{BodyExt, Full};
use hyper::{HeaderMap, Response};
use std::io::Write;

use crate::response::BoxBody;

/// これより小さいボディは圧縮しない(圧縮のオーバーヘッドが割に合わない)。
/// RPoem側の`middleware_hyper::COMPRESSION_MIN_SIZE`と同じ閾値。
const COMPRESSION_MIN_SIZE: usize = 256;

/// リクエストの`Accept-Encoding`ヘッダを見て、応答可能なら`gzip`で
/// レスポンスボディを圧縮する。既に`Content-Encoding`が付いている
/// (二重圧縮を避ける)・クライアントがgzipを受け付けない・ボディが
/// 小さすぎる、のいずれかに該当すれば無圧縮のまま返す。
///
/// `BoxBody = Full<Bytes>`はボディが既にメモリ上に全て揃っている型
/// だが、`Full`自体はバイト列を取り出す公開APIを持たないため
/// `BodyExt::collect`(このバイト列は既にメモリ上にあるので実際の
/// I/Oは発生せず即座に解決する)経由で取り出す。
// 圧縮を試みる対象のContent-Type(前方一致)。nginxの既定gzip_typesに
// 倣い、テキスト系・JSON・SVGのみを対象とする。動画・画像・バイナリは
// 対象外とする(圧縮効果が薄い上、ボディ全体をここでcollectしてしまうと
// proxy.rs側でのストリーミング転送化が無意味になるため、2026-08-15の
// 大容量動画ファイル配信高速化対応の一環としてここでも早期returnする)。
const COMPRESSIBLE_CONTENT_TYPES: &[&str] = &[
    "text/",
    "application/json",
    "application/javascript",
    "application/xml",
    "image/svg+xml",
];

pub async fn maybe_gzip(req_headers: &HeaderMap, resp: Response<BoxBody>) -> Response<BoxBody> {
    let accepts_gzip = req_headers
        .get(hyper::header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').any(|part| part.trim().starts_with("gzip")))
        .unwrap_or(false);
    if !accepts_gzip {
        return resp;
    }
    if resp.headers().contains_key(hyper::header::CONTENT_ENCODING) {
        return resp;
    }
    let is_compressible = resp
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| COMPRESSIBLE_CONTENT_TYPES.iter().any(|prefix| ct.starts_with(prefix)))
        .unwrap_or(false);
    if !is_compressible {
        return resp;
    }

    let (mut parts, body) = resp.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            tracing::warn!(error = %e, "failed to read response body for gzip compression");
            return Response::from_parts(parts, crate::response::full_body(Bytes::new()));
        }
    };
    if bytes.len() < COMPRESSION_MIN_SIZE {
        return Response::from_parts(parts, crate::response::full_body(bytes));
    }

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    let compressed = match encoder.write_all(&bytes).and_then(|_| encoder.finish()) {
        Ok(compressed) => compressed,
        // メモリ上のVec<u8>への書き込みが失敗することは通常無いが、
        // 万一失敗しても無圧縮のボディを返す(レスポンス自体を失わない)。
        Err(_) => return Response::from_parts(parts, crate::response::full_body(bytes)),
    };

    parts.headers.insert(hyper::header::CONTENT_ENCODING, "gzip".parse().unwrap());
    parts.headers.remove(hyper::header::CONTENT_LENGTH);
    Response::from_parts(parts, crate::response::full_body(Bytes::from(compressed)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderValue;
    use hyper::StatusCode;

    fn resp_with_body(body: Vec<u8>) -> Response<BoxBody> {
        Response::builder().status(StatusCode::OK).body(crate::response::full_body(Bytes::from(body))).unwrap()
    }

    fn resp_with_type(body: Vec<u8>, content_type: &str) -> Response<BoxBody> {
        Response::builder()
            .status(StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, content_type)
            .body(crate::response::full_body(Bytes::from(body)))
            .unwrap()
    }

    async fn body_bytes(resp: Response<BoxBody>) -> Vec<u8> {
        resp.into_body().collect().await.unwrap().to_bytes().to_vec()
    }

    fn headers_with_accept_encoding(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(hyper::header::ACCEPT_ENCODING, HeaderValue::from_str(value).unwrap());
        h
    }

    #[tokio::test]
    async fn compresses_large_repetitive_body_when_client_accepts_gzip() {
        let body = b"hello world hello world hello world ".repeat(20);
        let original_len = body.len();
        let resp = maybe_gzip(&headers_with_accept_encoding("gzip, deflate"), resp_with_type(body, "text/html; charset=utf-8")).await;
        assert_eq!(resp.headers().get(hyper::header::CONTENT_ENCODING).unwrap(), "gzip");
        let compressed = body_bytes(resp).await;
        assert!(compressed.len() < original_len, "compressed body should actually be smaller");
    }

    /// ストリーミング対応(2026-09-07)で、動画・画像・バイナリは圧縮対象外にした
    /// (本文を全部メモリに集めるとストリーミング配信が無意味になるため)。
    #[tokio::test]
    async fn does_not_compress_non_text_content_types_such_as_video() {
        let body = b"hello world hello world hello world ".repeat(20);
        let resp = maybe_gzip(&headers_with_accept_encoding("gzip"), resp_with_type(body.clone(), "video/mp4")).await;
        assert!(resp.headers().get(hyper::header::CONTENT_ENCODING).is_none());
        assert_eq!(body_bytes(resp).await, body, "body must be passed through untouched");
    }

    /// Content-Typeが無い応答は、圧縮対象かどうか判断できないので圧縮しない(安全側)。
    #[tokio::test]
    async fn does_not_compress_when_content_type_is_missing() {
        let body = b"hello world hello world hello world ".repeat(20);
        let resp = maybe_gzip(&headers_with_accept_encoding("gzip"), resp_with_body(body.clone())).await;
        assert!(resp.headers().get(hyper::header::CONTENT_ENCODING).is_none());
        assert_eq!(body_bytes(resp).await, body);
    }

    #[tokio::test]
    async fn does_not_compress_when_client_omits_gzip() {
        let body = b"hello world hello world hello world ".repeat(20);
        let resp = maybe_gzip(&HeaderMap::new(), resp_with_body(body.clone())).await;
        assert!(resp.headers().get(hyper::header::CONTENT_ENCODING).is_none());
        assert_eq!(body_bytes(resp).await, body);
    }

    #[tokio::test]
    async fn does_not_compress_small_bodies_even_when_client_accepts_gzip() {
        let body = b"tiny".to_vec();
        let resp = maybe_gzip(&headers_with_accept_encoding("gzip"), resp_with_body(body.clone())).await;
        assert!(resp.headers().get(hyper::header::CONTENT_ENCODING).is_none());
        assert_eq!(body_bytes(resp).await, body);
    }

    #[tokio::test]
    async fn does_not_double_compress_a_response_that_already_has_content_encoding() {
        let body = b"already-encoded-payload".repeat(50);
        let mut resp = resp_with_body(body.clone());
        resp.headers_mut().insert(hyper::header::CONTENT_ENCODING, HeaderValue::from_static("br"));
        let resp = maybe_gzip(&headers_with_accept_encoding("gzip"), resp).await;
        assert_eq!(resp.headers().get(hyper::header::CONTENT_ENCODING).unwrap(), "br");
        assert_eq!(body_bytes(resp).await, body);
    }
}
