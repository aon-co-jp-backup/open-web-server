//! `Idempotency-Key` ヘッダの必須化ミドルウェア。
//!
//! 課金アイテム・金融データを扱うエンドポイント (`/api/v1/items`, `/api/v1/transactions`)
//! では、クライアントが `Idempotency-Key` ヘッダを付けない限りリクエストを拒否する。
//! ボディ内の `idempotency_key` と併用することで、Gateway層・アプリ層の二重防御にする。

use poem::{Endpoint, Middleware, Request, Result};

pub struct IdempotencyGuard;

impl<E: Endpoint> Middleware<E> for IdempotencyGuard {
    type Output = IdempotencyGuardEndpoint<E>;

    fn transform(&self, ep: E) -> Self::Output {
        IdempotencyGuardEndpoint { ep }
    }
}

pub struct IdempotencyGuardEndpoint<E> {
    ep: E,
}

impl<E: Endpoint> Endpoint for IdempotencyGuardEndpoint<E> {
    type Output = E::Output;

    async fn call(&self, req: Request) -> Result<Self::Output> {
        let path = req.uri().path().to_string();
        let needs_key = path.starts_with("/api/v1/items") || path.starts_with("/api/v1/transactions");

        if needs_key && req.header("Idempotency-Key").is_none() {
            return Err(poem::Error::from_string(
                "Idempotency-Key header is required for mutating endpoints",
                poem::http::StatusCode::BAD_REQUEST,
            ));
        }

        self.ep.call(req).await
    }
}
