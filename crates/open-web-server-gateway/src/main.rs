//! open-web-server: エントリポイント
//!
//! tokio/hyper を直接用いた自前実装の HTTP Gateway (Web フレームワーク非依存)。
//! 3Dオンラインゲームのアイテム課金や金融データの読み書きを、24時間365日
//! ノンストップ・ミッションクリティカルな前提で受け付け、open-runo
//! (Federation Gateway) 経由で aruaru-db に届ける。
//!
//! ルーティング/ハンドラの API 形状は元々の Poem 実装と互換性を保ちつつ、
//! パッケージとしては Poem に依存しない (2026-07-10 スタック方針転換)。

mod app_proxy;
mod handlers;
mod middleware;
mod proxy;
mod response;
mod state;
mod telemetry;
mod tenant_router;

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
        (Method::POST, "/admin/tenants") => handlers::tenants::add_tenant(state, req).await,
        (Method::GET, "/admin/tenants") => handlers::tenants::list_tenants(state, &req).await,
        // `/tls`サフィックス付きのルートは、下の汎用`/admin/tenants/:host`
        // prefixマッチより先に評価する必要がある(先に評価されると
        // `:host`が`"foo.example.com/tls"`ごと拾われてしまうため)。
        (Method::POST, p) if p.starts_with("/admin/tenants/") && p.ends_with("/tls") => {
            let host = p
                .trim_start_matches("/admin/tenants/")
                .trim_end_matches("/tls")
                .to_string();
            handlers::tls::upsert_tenant_tls(state, req, &host).await
        }
        (Method::DELETE, p) if p.starts_with("/admin/tenants/") && p.ends_with("/tls") => {
            let host = p
                .trim_start_matches("/admin/tenants/")
                .trim_end_matches("/tls")
                .to_string();
            handlers::tls::remove_tenant_tls(state, &req, &host).await
        }
        (Method::DELETE, p) if p.starts_with("/admin/tenants/") => {
            let host = p.trim_start_matches("/admin/tenants/").to_string();
            handlers::tenants::remove_tenant(state, &req, &host).await
        }
        (Method::PUT, p) if p.starts_with("/admin/tenants/") => {
            let host = p.trim_start_matches("/admin/tenants/").to_string();
            handlers::tenants::update_tenant(state, req, &host).await
        }
        (Method::GET, "/healthz") => text_response(StatusCode::OK, "ok"),
        (Method::GET, p) if p.starts_with("/internal/db/state/") => {
            handlers::state_query::get_state_at_commit(state, p).await
        }
        // 上記いずれにも一致しないパスは、①複数ドメインを動的に振り分ける
        // マルチテナントルーティング(open-easyweb構想、`tenant_router`)を
        // まず試し、該当ドメイン登録が無ければ②単一アップストリームへの
        // 委譲(Apache+Tomcat型、`app_proxy`、`OPEN_WEB_SERVER_APP_UPSTREAM`)
        // にフォールバックする。どちらも該当しなければ従来通り`404`
        // (=アプリサーバー・マルチテナント設定のいずれも無くても単体で動作)。
        (_, _) => {
            let host_header = req
                .headers()
                .get(hyper::header::HOST)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);

            let tenant = match &host_header {
                Some(h) => state.tenants.resolve(h).await,
                None => None,
            };

            match tenant {
                Some(tenant) => proxy::forward_to(&tenant.config.backend_addr, req).await,
                None => match app_proxy::app_upstream_base() {
                    Some(base) => proxy::forward_to(&base, req).await,
                    None => text_response(StatusCode::NOT_FOUND, "not found"),
                },
            }
        }
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

/// TLS接続を受け付け続ける版の`accept_loop`。ルーティングロジック
/// (`route`/`dispatch`)はプレーンHTTPリスナーと完全に共有する——
/// 違いはハンドシェイク層(`TlsAcceptor`、`state.tls_resolver`によるSNI別
/// 証明書選択)のみ。ハンドシェイク失敗(不正クライアント・ポートスキャン等)
/// でリスナー自体を落とさない(既存`accept_loop`と同じ耐障害方針)。
async fn accept_tls_loop(
    listener: TcpListener,
    state: Arc<AppState>,
    acceptor: tokio_rustls::TlsAcceptor,
) -> anyhow::Result<()> {
    loop {
        let (stream, _peer_addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "failed to accept TLS connection");
                continue;
            }
        };

        let acceptor = acceptor.clone();
        let state = state.clone();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!(error = %e, "tls handshake failed");
                    return;
                }
            };
            let io = TokioIo::new(tls_stream);
            let service = service_fn(move |req| route(state.clone(), req));
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                tracing::warn!(error = %err, "tls connection error");
            }
        });
    }
}

/// `tls_task`が`Some`ならその完了を待ち、`None`なら永遠に解決しない
/// (`tokio::select!`で「TLSリスナー無効時はこの枝を無視する」を表現する
/// ための決して発火しないFuture——open-runo側の同種パターン
/// [`std::future::pending()`のバグ修正、2026-07-13]と同じ設計)。
async fn wait_optional_tls_task(
    task: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
) -> anyhow::Result<()> {
    match task {
        Some(handle) => handle.await.unwrap_or_else(|e| Err(anyhow::Error::from(e))),
        None => std::future::pending().await,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry_guard = telemetry::init()?;

    let state = Arc::new(AppState::from_env()?);
    state.load_domains_from_env().await?;

    let bind_addr: SocketAddr = std::env::var("OPEN_WEB_SERVER_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;
    tracing::info!(%bind_addr, "open-web-server listening");

    let listener = TcpListener::bind(bind_addr).await?;

    // TLS終端(`OPEN_WEB_SERVER_TLS_BIND`設定時のみ有効)。open-web-server
    // 自体がSNIに応じて複数テナントの証明書を切り替えられるようにする
    // 第一歩(2026-07-16、`docs/tls-tenant.md`参照)。証明書は
    // `POST /admin/tenants/:host/tls`で登録する(起動時点では未登録でも
    // 起動自体は失敗しない——証明書0件のリゾルバでリッスンだけ開始し、
    // ハンドシェイクは登録され次第成功するようになる)。
    let tls_task = match std::env::var("OPEN_WEB_SERVER_TLS_BIND").ok() {
        Some(tls_bind) => {
            let tls_addr: SocketAddr = tls_bind.parse()?;
            let tls_listener = TcpListener::bind(tls_addr).await?;
            tracing::info!(%tls_addr, "open-web-server TLS listening (per-tenant SNI cert resolution)");
            let server_config = open_web_server_wire::build_tenant_server_config(state.tls_resolver.clone());
            let acceptor = tokio_rustls::TlsAcceptor::from(server_config);
            Some(tokio::spawn(accept_tls_loop(tls_listener, state.clone(), acceptor)))
        }
        None => None,
    };

    let result = tokio::select! {
        res = accept_loop(listener, state) => res,
        res = wait_optional_tls_task(tls_task) => res,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutdown signal received");
            Ok(())
        }
    };

    // プロセス終了前にバッファ済みスパンを確実にフラッシュする。
    telemetry_guard.shutdown();

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // 本物のTLSハンドシェイクを検証するための「どんな証明書でも受け入れる」
    // テスト専用verifier(自己署名証明書のため信頼アンカーを持たない)。
    // 本番コードはこれを一切使わない——`open-web-server-wire::tls`の
    // テストモジュールにある`RecordingVerifier`と同じ理由・同じ実装
    // だが、こちらは証明書の中身自体は検証しない(ハンドシェイクが成功して
    // HTTPリクエストが本当にこのバイナリの`accept_tls_loop`まで届くことが
    // 目的であり、SNIごとの証明書選択自体は`open-web-server-wire`側の
    // `real_tls_handshake_resolves_different_cert_per_sni`で既に証明済み)。
    #[derive(Debug)]
    struct AcceptAnyCert;
    impl tokio_rustls::rustls::client::danger::ServerCertVerifier for AcceptAnyCert {
        fn verify_server_cert(
            &self,
            _end_entity: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[tokio_rustls::rustls::pki_types::CertificateDer<'_>],
            _server_name: &tokio_rustls::rustls::pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: tokio_rustls::rustls::pki_types::UnixTime,
        ) -> Result<tokio_rustls::rustls::client::danger::ServerCertVerified, tokio_rustls::rustls::Error> {
            Ok(tokio_rustls::rustls::client::danger::ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
            _dss: &tokio_rustls::rustls::DigitallySignedStruct,
        ) -> Result<tokio_rustls::rustls::client::danger::HandshakeSignatureValid, tokio_rustls::rustls::Error> {
            Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
            _dss: &tokio_rustls::rustls::DigitallySignedStruct,
        ) -> Result<tokio_rustls::rustls::client::danger::HandshakeSignatureValid, tokio_rustls::rustls::Error> {
            Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
            tokio_rustls::rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
        }
    }

    /// エンドツーエンド検証: (1) `POST /admin/tenants/:host/tls`で自己署名
    /// 証明書を登録 → (2) `accept_tls_loop`が実際にそのSNI名向けの
    /// TLSハンドシェイクに成功する → (3) TLS越しに`GET /healthz`を送ると
    /// 実際にこのバイナリの`dispatch()`まで届き200が返る、という
    /// open-web-server自体がApache+Nginx相当の自己完結TLS終端として
    /// 機能する最小構成を証明する(新規テストテナントのみが対象、
    /// 実運用中のaruaru.tokyo/audiocafe.tokyoのnginx設定には一切触れない)。
    #[tokio::test]
    async fn tls_admin_registration_enables_real_tls_handshake_and_dispatch() {
        use tokio_rustls::rustls::pki_types::ServerName;

        let state = Arc::new(AppState::from_env().expect("AppState::from_env should succeed with defaults"));

        // (1) 証明書を登録する。`hyper::body::Incoming`は本物のTCP接続
        // からしか作れないため、`POST /admin/tenants/:host/tls`
        // ハンドラ自体をこのテストでHTTP越しに叩くことはしない
        // (`handlers::tls::upsert_tenant_tls`は単に
        // `TenantCertResolver::upsert_pem`を呼ぶだけの薄いラッパーである
        // ことは実装を読めば自明——ハンドラのJSON解析/認証チェックの
        // 単体テストは別途`handlers::tls`側で行うべき関心事であり、ここでは
        // 「登録された証明書がTLSハンドシェイクに実際に反映されるか」を
        // 検証する)。実運用ではACME取得後にこの`upsert_pem`と同じ経路が
        // 呼ばれる。
        let cert = rcgen::generate_simple_self_signed(vec!["tls-test-tenant.example.test".to_string()]).unwrap();
        let cert_pem = cert.cert.pem();
        let key_pem = cert.key_pair.serialize_pem();
        state
            .tls_resolver
            .upsert_pem("tls-test-tenant.example.test", cert_pem.as_bytes(), key_pem.as_bytes())
            .expect("cert registration should succeed for a well-formed self-signed cert/key pair");

        // (2) + (3) 実TCP上で本物のTLSハンドシェイク+HTTPリクエスト。
        let server_config = open_web_server_wire::build_tenant_server_config(state.tls_resolver.clone());
        let acceptor = tokio_rustls::TlsAcceptor::from(server_config);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(accept_tls_loop(listener, state.clone(), acceptor));

        let client_config = tokio_rustls::rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(AcceptAnyCert))
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client_config));

        let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let server_name = ServerName::try_from("tls-test-tenant.example.test").unwrap();
        let tls_stream = connector.connect(server_name, tcp).await.expect("TLS handshake should succeed once a cert is registered for this SNI name");

        let io = hyper_util::rt::TokioIo::new(tls_stream);
        let (mut sender, connection) = hyper::client::conn::http1::handshake(io).await.unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });

        let request = Request::builder()
            .method(Method::GET)
            .uri("/healthz")
            .body(http_body_util::Empty::<bytes::Bytes>::new())
            .unwrap();
        let response = sender.send_request(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
