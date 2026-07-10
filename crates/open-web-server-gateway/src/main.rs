//! open-web-server: エントリポイント
//!
//! Poem を用いた HTTP/GraphQL Gateway。3Dオンラインゲームのアイテム課金や
//! 金融データの読み書きを、24時間365日ノンストップ・ミッションクリティカルな
//! 前提で受け付け、open-runo (Federation Gateway) 経由で aruaru-db に届ける。

mod handlers;
mod middleware;
mod state;
mod telemetry;

use poem::{
    listener::TcpListener, middleware::Tracing, EndpointExt, Route, Server,
};

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry_guard = telemetry::init()?;

    let state = AppState::from_env()?;

    let app = Route::new()
        .nest("/api/v1/items", handlers::items::routes())
        .nest("/api/v1/transactions", handlers::transactions::routes())
        .at("/healthz", poem::endpoint::make_sync(|_| "ok"))
        .with(Tracing)
        .with(middleware::idempotency::IdempotencyGuard)
        .data(state);

    let bind_addr = std::env::var("OPEN_WEB_SERVER_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    tracing::info!(%bind_addr, "open-web-server listening");

    let result = Server::new(TcpListener::bind(bind_addr)).run(app).await;

    // プロセス終了前にバッファ済みスパンを確実にフラッシュする。
    telemetry_guard.shutdown();

    result?;
    Ok(())
}
