//! 金融/クレジットカード決済トランザクション API
//!
//! 24時間365日ノンストップ運用を前提に、決済確定は
//! aruaru-db の Raft 分散合意 (open-runo 経由) が完了するまで確定としない。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;

use open_web_server_core::{IdempotencyKey, MutationRequest};

use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ChargeRequest {
    pub idempotency_key: String,
    pub account_id: String,
    pub amount_cents: i64,
    pub currency: String,
}

/// `POST /api/v1/transactions/charge`
#[tracing::instrument(
    name = "charge",
    skip(state, req),
    fields(account_id = tracing::field::Empty, amount_cents = tracing::field::Empty, currency = tracing::field::Empty)
)]
pub async fn charge(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    let body: ChargeRequest = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    let span = tracing::Span::current();
    span.record("account_id", body.account_id.as_str());
    span.record("amount_cents", body.amount_cents);
    span.record("currency", body.currency.as_str());

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
        Ok(receipt) => json_response(StatusCode::OK, &receipt),
        Err(e) => text_response(StatusCode::CONFLICT, e.to_string()),
    }
}
