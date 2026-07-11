//! HTTP レスポンス/リクエストボディの共通ヘルパー。
//!
//! `open-web-server-gateway` は Web フレームワークを使わず tokio/hyper を
//! 直接叩く自前実装のため、JSON のシリアライズ/デシリアライズや固定文字列
//! レスポンスの組み立てをここに集約し、各ハンドラから共通利用する。

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// このゲートウェイが返す全レスポンスのボディ型。
pub type BoxBody = Full<Bytes>;

/// 固定のテキストレスポンスを組み立てる。
pub fn text_response(status: StatusCode, body: impl Into<String>) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(body.into())))
        .expect("static response is always well-formed")
}

/// 値を JSON にシリアライズしてレスポンスを組み立てる。
pub fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response<BoxBody> {
    match serde_json::to_vec(value) {
        Ok(bytes) => Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(bytes)))
            .expect("json response is always well-formed"),
        Err(e) => text_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to serialize response: {e}"),
        ),
    }
}

/// リクエストボディを読み切って JSON としてデシリアライズする。
///
/// ボディの読み取り自体の失敗、あるいは JSON として不正な場合は
/// `400 Bad Request` レスポンスを `Err` として返す。
pub async fn read_json_body<T: DeserializeOwned>(
    req: Request<Incoming>,
) -> Result<T, Response<BoxBody>> {
    let bytes = match BodyExt::collect(req.into_body()).await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            return Err(text_response(
                StatusCode::BAD_REQUEST,
                format!("failed to read request body: {e}"),
            ))
        }
    };

    serde_json::from_slice(&bytes)
        .map_err(|e| text_response(StatusCode::BAD_REQUEST, format!("invalid JSON body: {e}")))
}
