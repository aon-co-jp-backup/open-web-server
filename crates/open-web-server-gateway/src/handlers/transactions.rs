//! 金融/クレジットカード決済トランザクション API
//!
//! 24時間365日ノンストップ運用を前提に、決済確定は
//! aruaru-db の Raft 分散合意 (open-runo 経由) が完了するまで確定としない。

use poem::{handler, web::Data, web::Json, IntoResponse, Response, Route};
use serde::Deserialize;

use open_web_server_core::{IdempotencyKey, MutationRequest};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ChargeRequest {
    pub idempotency_key: String,
    pub account_id: String,
    pub amount_cents: i64,
    pub currency: String,
}

#[handler]
pub async fn charge(Data(state): Data<&AppState>, Json(body): Json<ChargeRequest>) -> Response {
    let req = MutationRequest {
        idempotency_key: IdempotencyKey(body.idempotency_key),
        account_id: body.account_id,
        target: "financial_transactions".to_string(),
        payload: serde_json::json!({
            "amount_cents": body.amount_cents,
            "currency": body.currency,
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
    Route::new().at("/charge", poem::post(charge))
}
