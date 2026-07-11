//! 3Dオンラインゲームの課金アイテム API
//!
//! アイテム付与/購入は必ず `open-web-server-ledger` を通し、
//! aruaru-db への確定 (commit_id 取得) まで HTTP レスポンスを返さない。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;

use open_web_server_core::{IdempotencyKey, MutationRequest};

use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct GrantItemRequest {
    pub idempotency_key: String,
    pub account_id: String,
    pub item_id: String,
    pub quantity: u32,
}

/// `POST /api/v1/items/grant`
#[tracing::instrument(
    name = "grant_item",
    skip(state, req),
    fields(account_id = tracing::field::Empty, item_id = tracing::field::Empty, quantity = tracing::field::Empty)
)]
pub async fn grant_item(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    let body: GrantItemRequest = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    let span = tracing::Span::current();
    span.record("account_id", body.account_id.as_str());
    span.record("item_id", body.item_id.as_str());
    span.record("quantity", body.quantity as u64);

    let req = MutationRequest {
        idempotency_key: IdempotencyKey(body.idempotency_key),
        account_id: body.account_id,
        target: "game_items".to_string(),
        payload: serde_json::json!({
            "item_id": body.item_id,
            "quantity": body.quantity,
        }),
        requested_at: chrono::Utc::now(),
    };

    match state.ledger.commit(req).await {
        Ok(receipt) => json_response(StatusCode::OK, &receipt),
        Err(e) => text_response(StatusCode::CONFLICT, e.to_string()),
    }
}
