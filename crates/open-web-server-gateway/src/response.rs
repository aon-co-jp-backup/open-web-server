//! HTTP レスポンス/リクエストボディの共通ヘルパー。
//!
//! `open-web-server-gateway` は Web フレームワークを使わず tokio/hyper を
//! 直接叩く自前実装のため、JSON のシリアライズ/デシリアライズや固定文字列
//! レスポンスの組み立てをここに集約し、各ハンドラから共通利用する。

use bytes::Bytes;
use hyper::body::Body as HttpBody;
use http_body_util::combinators::BoxBody as HttpBoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::convert::Infallible;

/// レスポンスボディで発生しうるエラーを一枚岩の型へ束ねる(下流の
/// アップストリームエラーとメモリ上バッファのInfallibleを両方受け付ける)。
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// このゲートウェイが返す全レスポンスのボディ型。
///
/// 2026-08-15追記: 以前は固定(=送り返す前に全レスポンスを
/// メモリへ読み切ってから返す)だったため、大容量ファイル(動画等)を
/// 経由で返すと、クライアントへの送信開始前に全バイトの
/// メモリコピーが発生し数秒の遅延が生じていた(実際に aruaru.tokyo の
/// 544MBの動画で計測、直接ローカルloopback越しなら0.007秒のところ
/// このバッファリングのせいで3〜5.7秒かかっていた実例)。
/// による真のストリーミング対応
/// ボディへ変更し、(小さな固定レスポンス向け)と
/// (アップストリームからの転送向け、バッファ不要)の
/// 2つのコンストラクタを提供する。既存の呼び出し箇所は
/// 全てへ機械的に置換済み(挙動は完全に同一、単に型が
/// 変わっただけ)。
pub type BoxBody = HttpBoxBody<Bytes, BoxError>;

/// 固定バイト列(メモリ上に既に全て揃っているもの)からレスポンスボディを
/// 組み立てる。以前のと完全に同じ意味(即座に全データが
/// 利用可能)だが、型は新しい(ストリーミング対応)に揃える。
pub fn full_body(bytes: Bytes) -> BoxBody {
    Full::new(bytes)
        .map_err(|never: Infallible| -> BoxError { match never {} })
        .boxed()
}

/// アップストリーム(等)からのボディをバッファ
/// せずそのままストリーミング転送する。の大容量レスポンス
/// (動画等)転送で使う——による全読み込みを避ける
/// ことが目的。
pub fn stream_body<B>(body: B) -> BoxBody
where
    B: HttpBody<Data = Bytes> + Send + Sync + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    body.map_err(|e| Box::new(e) as BoxError).boxed()
}

/// 固定のテキストレスポンスを組み立てる。
pub fn text_response(status: StatusCode, body: impl Into<String>) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(full_body(Bytes::from(body.into())))
        .expect("static response is always well-formed")
}

/// 固定の HTML レスポンスを組み立てる(`/demo`のような読み取り専用の
/// 紹介・可視化ページ向け、2026-08-24新設)。
pub fn html_response(status: StatusCode, body: impl Into<String>) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header("content-type", "text/html; charset=utf-8")
        .body(full_body(Bytes::from(body.into())))
        .expect("static response is always well-formed")
}

/// 値を JSON にシリアライズしてレスポンスを組み立てる。
pub fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response<BoxBody> {
    match serde_json::to_vec(value) {
        Ok(bytes) => Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(full_body(Bytes::from(bytes)))
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

    sonic_rs::from_slice(&bytes)
        .map_err(|e| text_response(StatusCode::BAD_REQUEST, format!("invalid JSON body: {e}")))
}
