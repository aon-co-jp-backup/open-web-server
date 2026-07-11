//! `Idempotency-Key` ヘッダの必須化チェック。
//!
//! 課金アイテム・金融データを扱うエンドポイント (`/api/v1/items`, `/api/v1/transactions`)
//! では、クライアントが `Idempotency-Key` ヘッダを付けない限りリクエストを拒否する。
//! ボディ内の `idempotency_key` と併用することで、Gateway層・アプリ層の二重防御にする。
//!
//! Poem の `Middleware`/`Endpoint` トレイトには依存せず、ルーティング前に
//! 呼び出す素の関数として実装している (tokio/hyper 直接実装への移行)。
//! ボディ型に依存しない (`method`/`uri`/`headers` しか見ない) ため、
//! 本番の `Request<hyper::body::Incoming>` にもテスト用の `Request<()>` にも
//! そのまま使える。

use hyper::{Request, Response, StatusCode};

use crate::response::{text_response, BoxBody};

/// リクエストが `Idempotency-Key` ヘッダを必要とするパスに対して、
/// ヘッダが欠けていないかを確認する。
///
/// 欠けている場合は `400 Bad Request` レスポンスを `Err` として返す。
/// 呼び出し側 (ルータ) はこれを最終レスポンスとしてそのまま返す。
pub fn check<B>(req: &Request<B>) -> Result<(), Response<BoxBody>> {
    let path = req.uri().path();
    let needs_key = path.starts_with("/api/v1/items") || path.starts_with("/api/v1/transactions");

    if needs_key && !req.headers().contains_key("idempotency-key") {
        return Err(text_response(
            StatusCode::BAD_REQUEST,
            "Idempotency-Key header is required for mutating endpoints",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &str, idempotency_key: Option<&str>) -> Request<()> {
        let mut builder = Request::builder().method("POST").uri(path);
        if let Some(key) = idempotency_key {
            builder = builder.header("Idempotency-Key", key);
        }
        builder.body(()).unwrap()
    }

    #[test]
    fn rejects_mutating_request_without_idempotency_key() {
        let req = request("/api/v1/items/grant", None);
        assert!(check(&req).is_err());
    }

    #[test]
    fn allows_mutating_request_with_idempotency_key() {
        let req = request(
            "/api/v1/items/grant",
            Some("11111111-1111-1111-1111-111111111111"),
        );
        assert!(check(&req).is_ok());
    }

    #[test]
    fn allows_non_mutating_request_without_idempotency_key() {
        let req = request("/healthz", None);
        assert!(check(&req).is_ok());
    }
}
