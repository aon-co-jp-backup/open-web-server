//! 3Dオンラインゲームの課金アイテム API
//!
//! アイテム付与/購入は必ず `open-web-server-ledger` を通し、
//! aruaru-db への確定 (commit_id 取得) まで HTTP レスポンスを返さない。

use poem::{handler, web::Json, web::Data, Route, IntoResponse, Response};
use serde::Deserialize;

use open_web_server_core::{IdempotencyKey, MutationRequest};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct GrantItemRequest {
    pub idempotency_key: String,
    pub account_id: String,
    pub item_id: String,
    pub quantity: u32,
}

#[handler]
#[tracing::instrument(
    name = "grant_item",
    skip(state, body),
    fields(account_id = %body.account_id, item_id = %body.item_id, quantity = body.quantity)
)]
pub async fn grant_item(Data(state): Data<&AppState>, Json(body): Json<GrantItemRequest>) -> Response {
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
        Ok(receipt) => Json(receipt).into_response(),
        Err(e) => poem::Error::from_string(e.to_string(), poem::http::StatusCode::CONFLICT)
            .into_response(),
    }
}

pub fn routes() -> Route {
    Route::new().at("/grant", poem::post(grant_item))
}
