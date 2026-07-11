//! open-web-server: エントリポイント
//!
//! tokio/hyper を直接用いた自前実装の HTTP Gateway (Web フレームワーク非依存)。
//! 3Dオンラインゲームのアイテム課金や金融データの読み書きを、24時間365日
//! ノンストップ・ミッションクリティカルな前提で受け付け、open-runo
//! (Federation Gateway) 経由で aruaru-db に届ける。
//!
//! ルーティング/ハンドラの API 形状は元々の Poem 実装と互換性を保ちつつ、
//! パッケージとしては Poem に依存しない (2026-07-10 スタック方針転換)。

mod handlers;
mod middleware;
mod response;
mod state;
mod telemetry;

use std::net::SocketAddr;
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tracing::Instrument;

use response::{text_response, BoxBody};
use state::AppState;

/// パス/メソッドに応じてハンドラへディスパッチする。
///
/// `Idempotency-Key` ヘッダの必須化チェックはここでルーティングより先に行う
/// (元 Poem 実装の `IdempotencyGuard` ミドルウェアと同等の位置づけ)。
async fn dispatch(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = middleware::idempotency::check(&req) {
        return resp;
    }

    let method = req.method().clone();
    let path = req.uri().path().to_string();

    match (method, path.as_str()) {
        (Method::POST, "/api/v1/items/grant") => handlers::items::grant_item(state, req).await,
        (Method::POST, "/api/v1/transactions/charge") => {
            handlers::transactions::charge(state, req).await
        }
        (Method::GET, "/healthz") => text_response(StatusCode::OK, "ok"),
        _ => text_response(StatusCode::NOT_FOUND, "not found"),
    }
}

/// 1リクエスト分の処理を、リクエストログ用の `tracing` スパンで包む。
///
/// 元 Poem 実装の `poem::middleware::Tracing` に相当する、method/path/status/
/// 所要時間を記録するリクエストロギング層。
async fn route(
    state: Arc<AppState>,
    req: Request<Incoming>,
) -> Result<Response<BoxBody>, std::convert::Infallible> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let span = tracing::info_span!("http_request", %method, %path, status = tracing::field::Empty);

    async move {
        let started = std::time::Instant::now();
        let response = dispatch(state, req).await;

        tracing::Span::current().record("status", response.status().as_u16());
        tracing::info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            "request completed"
        );

        Ok(response)
    }
    .instrument(span)
    .await
}

/// TCP接続を受け付け続け、1接続ごとに HTTP/1.1 サーバを `spawn` する。
async fn accept_loop(listener: TcpListener, state: Arc<AppState>) -> anyhow::Result<()> {
    loop {
        let (stream, _peer_addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "failed to accept connection");
                continue;
            }
        };

        let io = TokioIo::new(stream);
        let state = state.clone();

        tokio::spawn(async move {
            let service = service_fn(move |req| route(state.clone(), req));
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                tracing::warn!(error = %err, "connection error");
            }
        });
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry_guard = telemetry::init()?;

    let state = Arc::new(AppState::from_env()?);

    let bind_addr: SocketAddr = std::env::var("OPEN_WEB_SERVER_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;
    tracing::info!(%bind_addr, "open-web-server listening");

    let listener = TcpListener::bind(bind_addr).await?;

    let result = tokio::select! {
        res = accept_loop(listener, state) => res,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutdown signal received");
            Ok(())
        }
    };

    // プロセス終了前にバッファ済みスパンを確実にフラッシュする。
    telemetry_guard.shutdown();

    result
}
