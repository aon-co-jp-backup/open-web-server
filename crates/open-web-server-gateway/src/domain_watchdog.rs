//! ドメイン/URL死活監視 + 自動復旧(watchdog)。
//!
//! 2026-07-29のTLS証明書ディレクトリ消失事故(`tls-certs`が実際には
//! 書き込まれておらず、`systemctl restart`で稼働中の全ドメインが同時に
//! TLSハンドシェイク失敗した実障害、詳細は`CLAUDE.md` HANDOFF参照)を
//! 受けて、ユーザー指示「登録したはずのドメイン・URL等が正常に表示
//! されているか自動で定期的に確認する機能と、aruaru-llmによるAI判断で
//! 自動で復活する機能」に対応する第一実装。
//!
//! **スコープの正直な明記**:
//! 1. 監視対象は`TenantRegistry`/`WebVhostRegistry`に登録済みのホスト名
//!    のみ(未登録の外部URLは対象外)。
//! 2. 「AI判断」は`aruaru-llm`(到達可能な場合のみ、`AiAdvisor`トレイト
//!    経由でopt-in)へ障害内容を送り診断コメントを取得・記録する
//!    **助言的な役割**に留める。aruaru-llmの応答をそのまま復旧コマンド
//!    として実行するような設計にはしていない(誤診断による誤操作を
//!    避けるため——`state`欄の`last_ai_diagnosis`として記録・可視化する
//!    だけで、実際の復旧アクションの判断には使わない)。
//! 3. 実際の自動復旧は現時点で「TLSハンドシェイク失敗
//!    (`CheckOutcome.tls_broken`)→該当ホストのACME証明書再取得
//!    (`CertReissuer`)」の1パターンのみ——今回の実障害と直接対応する
//!    最小実装。バックエンド到達不能(502)等、他の障害への自動復旧は
//!    今回のスコープ外のまま(次回拡張候補)。
//! 4. 死活チェック自体はこのプロセス自身のTLSポートへの実TCP+TLS
//!    ハンドシェイク+簡易HTTP GETであり、外部から見た本当の到達性
//!    (DNS・外部ネットワーク経路)までは検証しない——あくまで
//!    「このプロセスが該当ホスト向けに正しく応答できる状態にあるか」の
//!    自己診断。
//!
//! **コンテンツ確認(2026-07-29追加)**: audiocafe.tokyoで実際に発生した
//! 「コードはpush済みだがVPSへ再デプロイしておらず、本番ページには
//! 反映されていなかった(SPEC/PASS LABSリンクが実際には表示されて
//! いなかった)」という実障害を受けて、単なる疎通確認(HTTPステータス)
//! だけでなく、**ページ本文に期待する文字列(ボタンのラベル・リンク先URL
//! 等)が実際に含まれているか**まで確認できるようにした
//! (`ContentExpectations`/`WatchdogState::set_expectations`)。
//! 期待文字列が1つでも本文に見つからない場合は「異常」として扱い、
//! 通常のTLS障害と同じ失敗カウント・AI診断のパイプラインに乗る——
//! ただしコンテンツ欠落はACME再取得では直せないため、**自動復旧の対象
//! にはせず、検知・記録・AI診断による助言のみに留める**(正しい再デプロイ
//! 手順は状況依存であり、確認なしに任意のコマンドを自動実行すべきでは
//! ないため)。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

/// 1ホスト分の直近の死活状態。
#[derive(Debug, Clone, Default)]
pub struct HostHealth {
    pub consecutive_failures: u32,
    pub last_checked_unix: u64,
    pub last_ok: bool,
    pub last_detail: String,
    pub last_recovery_action: Option<String>,
    pub last_ai_diagnosis: Option<String>,
}

/// 全ホストの死活状態を保持する共有ステート(`AppState`から常時参照可能)。
#[derive(Debug, Default)]
pub struct WatchdogState {
    hosts: RwLock<HashMap<String, HostHealth>>,
    /// ホストごとに「本文に含まれているべき文字列」の一覧(空/未登録なら
    /// コンテンツ確認は行わずHTTPステータスのみで判定、既存動作と後方
    /// 互換)。
    expectations: RwLock<HashMap<String, Vec<String>>>,
}

impl WatchdogState {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn snapshot(&self) -> Vec<(String, HostHealth)> {
        self.hosts
            .read()
            .await
            .iter()
            .map(|(h, v)| (h.clone(), v.clone()))
            .collect()
    }

    /// 指定ホストの本文に含まれているべき文字列一覧を設定する(既存の
    /// 一覧は置き換え、空配列を渡せば「確認を行わない」に戻る)。
    pub async fn set_expectations(&self, host: &str, expected: Vec<String>) {
        if expected.is_empty() {
            self.expectations.write().await.remove(host);
        } else {
            self.expectations
                .write()
                .await
                .insert(host.to_string(), expected);
        }
    }

    pub async fn expectations_for(&self, host: &str) -> Vec<String> {
        self.expectations
            .read()
            .await
            .get(host)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn all_expectations(&self) -> Vec<(String, Vec<String>)> {
        self.expectations
            .read()
            .await
            .iter()
            .map(|(h, v)| (h.clone(), v.clone()))
            .collect()
    }

    async fn failures_for(&self, host: &str) -> u32 {
        self.hosts
            .read()
            .await
            .get(host)
            .map(|h| h.consecutive_failures)
            .unwrap_or(0)
    }

    async fn record(&self, host: &str, outcome: &CheckOutcome) {
        let mut guard = self.hosts.write().await;
        let entry = guard.entry(host.to_string()).or_default();
        entry.last_checked_unix = now_unix();
        entry.last_ok = outcome.ok;
        entry.last_detail = outcome.detail.clone();
        if outcome.ok {
            entry.consecutive_failures = 0;
            entry.last_recovery_action = None;
            entry.last_ai_diagnosis = None;
        } else {
            entry.consecutive_failures = entry.consecutive_failures.saturating_add(1);
        }
    }

    async fn set_recovery_action(&self, host: &str, action: String) {
        if let Some(e) = self.hosts.write().await.get_mut(host) {
            e.last_recovery_action = Some(action);
        }
    }

    async fn set_ai_diagnosis(&self, host: &str, diagnosis: String) {
        if let Some(e) = self.hosts.write().await.get_mut(host) {
            e.last_ai_diagnosis = Some(diagnosis);
        }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 1回分の死活チェック結果。
#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub ok: bool,
    pub detail: String,
    /// TLSハンドシェイク自体が失敗した(=証明書が無い/壊れている疑いが
    /// 強い)場合に`true`。HTTPレベルの失敗(404等)とは区別し、自動復旧
    /// (ACME再取得)を発火させるかどうかの判定に使う。
    pub tls_broken: bool,
    /// 取得できたレスポンス本文(コンテンツ確認用、取得できなかった場合は
    /// 空文字列)。メモリ保護のため先頭256KiBまでに切り詰める。
    pub body: String,
}

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
        tokio_rustls::rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// 自プロセスのTLSポートへ実際にTLSハンドシェイク+簡易HTTP GETを行い、
/// 到達可能かを判定する(証明書のCA検証はしない——「自分自身のサーバーに
/// 正しいSNI証明書付きで到達できるか」という自己診断が目的であり、外部の
/// 信頼チェーン検証は目的外)。
pub async fn check_host_https(host: &str, tls_addr: SocketAddr) -> CheckOutcome {
    check_host_https_path(host, tls_addr, "/").await
}

/// [`check_host_https`]の任意パス指定版(コンテンツ確認は基本`/`だけで
/// 十分だが、`/index.php`のような特定パスだけ登録されたホストの確認にも
/// 使えるようにする)。
pub async fn check_host_https_path(host: &str, tls_addr: SocketAddr, path: &str) -> CheckOutcome {
    use tokio_rustls::rustls::pki_types::ServerName;

    const MAX_BODY_BYTES: usize = 256 * 1024;

    let tcp = match tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect(tls_addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return CheckOutcome { ok: false, detail: format!("tcp connect failed: {e}"), tls_broken: true, body: String::new() };
        }
        Err(_) => {
            return CheckOutcome { ok: false, detail: "tcp connect timed out".to_string(), tls_broken: true, body: String::new() };
        }
    };

    let client_config = tokio_rustls::rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCert))
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));

    let server_name = match ServerName::try_from(host.to_string()) {
        Ok(n) => n,
        Err(e) => {
            return CheckOutcome { ok: false, detail: format!("invalid SNI host name: {e}"), tls_broken: true, body: String::new() };
        }
    };

    let tls_stream = match tokio::time::timeout(Duration::from_secs(5), connector.connect(server_name, tcp)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            return CheckOutcome { ok: false, detail: format!("tls handshake failed: {e}"), tls_broken: true, body: String::new() };
        }
        Err(_) => {
            return CheckOutcome { ok: false, detail: "tls handshake timed out".to_string(), tls_broken: true, body: String::new() };
        }
    };

    let io = hyper_util::rt::TokioIo::new(tls_stream);
    let (mut sender, connection) = match hyper::client::conn::http1::handshake(io).await {
        Ok(v) => v,
        Err(e) => {
            return CheckOutcome { ok: false, detail: format!("http handshake failed: {e}"), tls_broken: false, body: String::new() };
        }
    };
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let request = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(path)
        .header("host", host)
        .body(http_body_util::Empty::<bytes::Bytes>::new())
        .expect("well-formed request");

    match tokio::time::timeout(Duration::from_secs(5), sender.send_request(request)).await {
        Ok(Ok(resp)) => {
            let status = resp.status();
            // 4xx(該当パス自体が無いだけ、"/index.php"限定登録のような
            // ホストは"/"に404を返し得る)まではサーバー自体は生きている
            // とみなす。5xxのみ異常扱い。
            let ok = !status.is_server_error();
            use http_body_util::BodyExt;
            let body = match tokio::time::timeout(Duration::from_secs(5), resp.collect()).await {
                Ok(Ok(collected)) => {
                    let bytes = collected.to_bytes();
                    let truncated = &bytes[..bytes.len().min(MAX_BODY_BYTES)];
                    String::from_utf8_lossy(truncated).to_string()
                }
                _ => String::new(),
            };
            CheckOutcome { ok, detail: format!("http status {status}"), tls_broken: false, body }
        }
        Ok(Err(e)) => {
            CheckOutcome { ok: false, detail: format!("http request failed: {e}"), tls_broken: false, body: String::new() }
        }
        Err(_) => {
            CheckOutcome { ok: false, detail: "http request timed out".to_string(), tls_broken: false, body: String::new() }
        }
    }
}

/// 復旧アクション(テスト時にダミー実装へ差し替えるためトレイト化)。
#[async_trait::async_trait]
pub trait CertReissuer: Send + Sync {
    async fn reissue(&self, host: &str) -> anyhow::Result<()>;
}

/// AI診断(aruaru-llm等、到達可能な場合のみ・助言目的のみ、モジュールdoc
/// 参照)。
#[async_trait::async_trait]
pub trait AiAdvisor: Send + Sync {
    async fn diagnose(&self, host: &str, detail: &str) -> Option<String>;
}

/// 実運用向けの`AiAdvisor`実装: `aruaru-llm`の`POST /v1/chat`へ問い合わせる。
/// 到達不能・タイムアウトはエラーにせず`None`を返す(補助的診断のため
/// 権威パスをブロックしない、既存の`DbStateReader`等と同じ設計方針)。
#[cfg(feature = "domain-watchdog")]
pub struct AruaruLlmAdvisor {
    endpoint: String,
    client: reqwest::Client,
}

#[cfg(feature = "domain-watchdog")]
impl AruaruLlmAdvisor {
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
        }
    }
}

#[cfg(feature = "domain-watchdog")]
#[async_trait::async_trait]
impl AiAdvisor for AruaruLlmAdvisor {
    async fn diagnose(&self, host: &str, detail: &str) -> Option<String> {
        let message = format!(
            "ドメイン{host}のヘルスチェックで異常を検知しました。詳細: {detail}。\
             考えられる原因と対応を日本語で一言(50文字程度)で教えてください。"
        );
        let url = format!("{}/v1/chat", self.endpoint.trim_end_matches('/'));
        let resp = tokio::time::timeout(
            Duration::from_secs(5),
            self.client
                .post(&url)
                .json(&serde_json::json!({ "message": message, "tenant": "open-web-server-watchdog" }))
                .send(),
        )
        .await
        .ok()?
        .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body: serde_json::Value = resp.json().await.ok()?;
        body.get("reply")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }
}

/// 監視ループ本体。
pub struct DomainWatchdog {
    pub state: Arc<WatchdogState>,
    pub tls_bind: SocketAddr,
    /// この回数連続で失敗したら自動復旧を試みる。
    pub failure_threshold: u32,
    pub cert_reissuer: Option<Arc<dyn CertReissuer>>,
    pub ai_advisor: Option<Arc<dyn AiAdvisor>>,
}

impl DomainWatchdog {
    /// 与えられたホスト一覧を1巡だけチェックする(テスト・単発実行用)。
    pub async fn run_once(&self, hosts: &[String]) {
        for host in hosts {
            let mut outcome = check_host_https(host, self.tls_bind).await;

            // コンテンツ確認(本文に含まれているべき文字列、モジュールdoc
            // 参照): HTTPレベルでは"正常"でも、期待する文字列が本文に
            // 見つからなければ異常として扱う。TLS/HTTP自体は生きている
            // ため`tls_broken`は変更しない(=自動証明書再取得の対象には
            // ならない、コンテンツ欠落はそれで直る問題ではないため)。
            if outcome.ok {
                let expected = self.state.expectations_for(host).await;
                if !expected.is_empty() {
                    let missing: Vec<&String> = expected.iter().filter(|s| !outcome.body.contains(s.as_str())).collect();
                    if !missing.is_empty() {
                        outcome.ok = false;
                        outcome.detail = format!(
                            "content check failed: missing expected text(s): {}",
                            missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                        );
                    }
                }
            }

            self.state.record(host, &outcome).await;

            if outcome.ok {
                continue;
            }

            let failures = self.state.failures_for(host).await;
            if failures < self.failure_threshold {
                continue;
            }

            if let Some(advisor) = &self.ai_advisor {
                if let Some(diagnosis) = advisor.diagnose(host, &outcome.detail).await {
                    self.state.set_ai_diagnosis(host, diagnosis).await;
                }
            }

            if outcome.tls_broken {
                if let Some(reissuer) = &self.cert_reissuer {
                    match reissuer.reissue(host).await {
                        Ok(()) => {
                            tracing::info!(
                                host,
                                "domain_watchdog: automatically reissued TLS certificate after repeated failures"
                            );
                            self.state
                                .set_recovery_action(host, "tls_reissued".to_string())
                                .await;
                        }
                        Err(e) => {
                            tracing::warn!(host, error = %e, "domain_watchdog: automatic TLS reissuance attempt failed");
                            self.state
                                .set_recovery_action(host, format!("tls_reissue_failed: {e}"))
                                .await;
                        }
                    }
                }
            }
        }
    }

    /// 定期実行ループ(本番起動用)。`hosts_provider`は毎回呼び出される
    /// クロージャで、その時点で登録済みのホスト一覧を返す(`tenants`/
    /// `web_vhosts`が動的に増減するため、ループ開始時に固定リストとして
    /// キャプチャしない設計)。
    pub async fn run_loop<F, Fut>(self: Arc<Self>, interval: Duration, hosts_provider: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Vec<String>> + Send,
    {
        loop {
            let hosts = hosts_provider().await;
            self.run_once(&hosts).await;
            tokio::time::sleep(interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn check_host_https_reports_tls_broken_when_nothing_is_listening() {
        // ポート0を実際にbindしてから即座に閉じ、確実に何も listen していない
        // アドレスを用意する(OSがそのポートをすぐ再利用しない保証は無いが、
        // "connection refused"系のエラーは確実に発生する)。
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let outcome = check_host_https("nobody-is-listening.example.test", addr).await;
        assert!(!outcome.ok);
        assert!(outcome.tls_broken);
    }

    #[tokio::test]
    async fn check_host_https_succeeds_against_a_real_tls_listener() {
        use tokio_rustls::TlsAcceptor;

        let resolver = open_web_server_wire::TenantCertResolver::new();
        let cert = rcgen::generate_simple_self_signed(vec!["watchdog-test.example.test".to_string()]).unwrap();
        resolver
            .upsert_pem(
                "watchdog-test.example.test",
                cert.cert.pem().as_bytes(),
                cert.key_pair.serialize_pem().as_bytes(),
            )
            .unwrap();

        let server_config = open_web_server_wire::build_tenant_server_config(resolver);
        let acceptor = TlsAcceptor::from(server_config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    if let Ok(tls_stream) = acceptor.accept(stream).await {
                        let io = hyper_util::rt::TokioIo::new(tls_stream);
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(
                                io,
                                hyper::service::service_fn(|_req: hyper::Request<hyper::body::Incoming>| async move {
                                    Ok::<_, std::convert::Infallible>(hyper::Response::new(
                                        http_body_util::Full::new(bytes::Bytes::from_static(b"ok")),
                                    ))
                                }),
                            )
                            .await;
                    }
                });
            }
        });

        let outcome = check_host_https("watchdog-test.example.test", addr).await;
        assert!(outcome.ok, "expected ok, got {outcome:?}");
        assert!(!outcome.tls_broken);
    }

    struct CountingReissuer {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl CertReissuer for CountingReissuer {
        async fn reissue(&self, _host: &str) -> anyhow::Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct StubAdvisor {
        diagnosis: String,
    }

    #[async_trait::async_trait]
    impl AiAdvisor for StubAdvisor {
        async fn diagnose(&self, _host: &str, _detail: &str) -> Option<String> {
            Some(self.diagnosis.clone())
        }
    }

    #[tokio::test]
    async fn repeated_tls_failures_trigger_automatic_reissuance_and_ai_diagnosis() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let calls = Arc::new(AtomicUsize::new(0));
        let watchdog = DomainWatchdog {
            state: Arc::new(WatchdogState::new()),
            tls_bind: addr,
            failure_threshold: 2,
            cert_reissuer: Some(Arc::new(CountingReissuer { calls: calls.clone() })),
            ai_advisor: Some(Arc::new(StubAdvisor {
                diagnosis: "証明書が見つからない可能性があります".to_string(),
            })),
        };

        let hosts = vec!["broken-host.example.test".to_string()];

        // 1回目: まだ閾値未満のため復旧は発火しない。
        watchdog.run_once(&hosts).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // 2回目: 閾値(2)に到達し、自動復旧+AI診断が記録される。
        watchdog.run_once(&hosts).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let snapshot = watchdog.state.snapshot().await;
        let (_, health) = snapshot
            .into_iter()
            .find(|(h, _)| h == "broken-host.example.test")
            .unwrap();
        assert_eq!(health.consecutive_failures, 2);
        assert_eq!(health.last_recovery_action.as_deref(), Some("tls_reissued"));
        assert!(health.last_ai_diagnosis.is_some());
    }

    #[tokio::test]
    async fn recovery_resets_failure_count_and_clears_recovery_state() {
        use tokio_rustls::TlsAcceptor;

        let resolver = open_web_server_wire::TenantCertResolver::new();
        let cert = rcgen::generate_simple_self_signed(vec!["recovers.example.test".to_string()]).unwrap();
        resolver
            .upsert_pem(
                "recovers.example.test",
                cert.cert.pem().as_bytes(),
                cert.key_pair.serialize_pem().as_bytes(),
            )
            .unwrap();
        let server_config = open_web_server_wire::build_tenant_server_config(resolver);
        let acceptor = TlsAcceptor::from(server_config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    if let Ok(tls_stream) = acceptor.accept(stream).await {
                        let io = hyper_util::rt::TokioIo::new(tls_stream);
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(
                                io,
                                hyper::service::service_fn(|_req: hyper::Request<hyper::body::Incoming>| async move {
                                    Ok::<_, std::convert::Infallible>(hyper::Response::new(
                                        http_body_util::Full::new(bytes::Bytes::from_static(b"ok")),
                                    ))
                                }),
                            )
                            .await;
                    }
                });
            }
        });

        let watchdog = DomainWatchdog {
            state: Arc::new(WatchdogState::new()),
            tls_bind: addr,
            failure_threshold: 1,
            cert_reissuer: None,
            ai_advisor: None,
        };

        // 先に人工的に失敗を1件記録してから正常化する。
        watchdog
            .state
            .record(
                "recovers.example.test",
                &CheckOutcome { ok: false, detail: "manual failure injection".to_string(), tls_broken: true, body: String::new() },
            )
            .await;
        watchdog
            .state
            .set_recovery_action("recovers.example.test", "tls_reissued".to_string())
            .await;

        watchdog.run_once(&["recovers.example.test".to_string()]).await;

        let snapshot = watchdog.state.snapshot().await;
        let (_, health) = snapshot
            .into_iter()
            .find(|(h, _)| h == "recovers.example.test")
            .unwrap();
        assert_eq!(health.consecutive_failures, 0);
        assert!(health.last_ok);
        assert!(health.last_recovery_action.is_none());
    }

    /// audiocafe.tokyoで実際に起きた「HTTPは200だが、コードをまだ
    /// デプロイしていないため期待するリンクが本文に無い」障害パターンを
    /// 再現する。TLS/HTTP自体は正常(`tls_broken`にはならない)ため
    /// 自動証明書再取得は発火しないが、`ok=false`として記録され、AI診断は
    /// 呼ばれることを確認する。
    #[tokio::test]
    async fn missing_expected_content_is_treated_as_a_failure_without_triggering_cert_reissuance() {
        use tokio_rustls::TlsAcceptor;

        let resolver = open_web_server_wire::TenantCertResolver::new();
        let cert = rcgen::generate_simple_self_signed(vec!["content-check.example.test".to_string()]).unwrap();
        resolver
            .upsert_pem(
                "content-check.example.test",
                cert.cert.pem().as_bytes(),
                cert.key_pair.serialize_pem().as_bytes(),
            )
            .unwrap();
        let server_config = open_web_server_wire::build_tenant_server_config(resolver);
        let acceptor = TlsAcceptor::from(server_config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    if let Ok(tls_stream) = acceptor.accept(stream).await {
                        let io = hyper_util::rt::TokioIo::new(tls_stream);
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(
                                io,
                                hyper::service::service_fn(|_req: hyper::Request<hyper::body::Incoming>| async move {
                                    // "SPEC RPA-MG1000"を含まない、まだ古いままのページ本文。
                                    Ok::<_, std::convert::Infallible>(hyper::Response::new(
                                        http_body_util::Full::new(bytes::Bytes::from_static(b"<html>old content</html>")),
                                    ))
                                }),
                            )
                            .await;
                    }
                });
            }
        });

        let calls = Arc::new(AtomicUsize::new(0));
        let watchdog = DomainWatchdog {
            state: Arc::new(WatchdogState::new()),
            tls_bind: addr,
            failure_threshold: 1,
            cert_reissuer: Some(Arc::new(CountingReissuer { calls: calls.clone() })),
            ai_advisor: Some(Arc::new(StubAdvisor {
                diagnosis: "デプロイし忘れの可能性があります".to_string(),
            })),
        };
        watchdog
            .state
            .set_expectations("content-check.example.test", vec!["SPEC RPA-MG1000".to_string()])
            .await;

        watchdog.run_once(&["content-check.example.test".to_string()]).await;

        let snapshot = watchdog.state.snapshot().await;
        let (_, health) = snapshot
            .into_iter()
            .find(|(h, _)| h == "content-check.example.test")
            .unwrap();
        assert!(!health.last_ok);
        assert!(health.last_detail.contains("SPEC RPA-MG1000"));
        assert!(health.last_ai_diagnosis.is_some());
        // コンテンツ欠落はTLS障害ではないため、自動証明書再取得は発火しない。
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(health.last_recovery_action.is_none());
    }

    #[tokio::test]
    async fn content_check_passes_when_expected_text_is_present() {
        use tokio_rustls::TlsAcceptor;

        let resolver = open_web_server_wire::TenantCertResolver::new();
        let cert = rcgen::generate_simple_self_signed(vec!["content-ok.example.test".to_string()]).unwrap();
        resolver
            .upsert_pem(
                "content-ok.example.test",
                cert.cert.pem().as_bytes(),
                cert.key_pair.serialize_pem().as_bytes(),
            )
            .unwrap();
        let server_config = open_web_server_wire::build_tenant_server_config(resolver);
        let acceptor = TlsAcceptor::from(server_config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    if let Ok(tls_stream) = acceptor.accept(stream).await {
                        let io = hyper_util::rt::TokioIo::new(tls_stream);
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(
                                io,
                                hyper::service::service_fn(|_req: hyper::Request<hyper::body::Incoming>| async move {
                                    Ok::<_, std::convert::Infallible>(hyper::Response::new(
                                        http_body_util::Full::new(bytes::Bytes::from_static(
                                            b"<html>SPEC RPA-MG1000 is here</html>",
                                        )),
                                    ))
                                }),
                            )
                            .await;
                    }
                });
            }
        });

        let watchdog = DomainWatchdog {
            state: Arc::new(WatchdogState::new()),
            tls_bind: addr,
            failure_threshold: 1,
            cert_reissuer: None,
            ai_advisor: None,
        };
        watchdog
            .state
            .set_expectations("content-ok.example.test", vec!["SPEC RPA-MG1000".to_string()])
            .await;

        watchdog.run_once(&["content-ok.example.test".to_string()]).await;

        let snapshot = watchdog.state.snapshot().await;
        let (_, health) = snapshot
            .into_iter()
            .find(|(h, _)| h == "content-ok.example.test")
            .unwrap();
        assert!(health.last_ok);
    }
}
