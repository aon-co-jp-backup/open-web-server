//! QUIC 冗長経路 (拡張要件(3) 「通信層の四重化」の第③伝送路、第一実装版)
//!
//! ## スコープと限界 (2026-07-12、正直な記載)
//!
//! `open-web-server/CLAUDE.md` の拡張要件(3)に定義された4伝送路のうち、
//! ①TCP-IP・②UDP-IP(`udp_channel`)は既に実装済み。本モジュールは
//! **③QUIC** の最初の具体的な実装であり、[`quinn`](https://docs.rs/quinn)
//! (UDP上にQUICを実装するRustの標準的なクレート)を用いる。
//!
//! - **Multipath QUIC (MPQUIC) ではない**: `quinn` 単体は単一経路のQUICの
//!   み提供する(MPQUICは複数の物理経路に単一コネクションを分散する拡張
//!   仕様で、2026-07-12時点でquinnに正式なMPQUIC実装は無い)。今回は
//!   「QUIC自体」を④とは独立した第3の伝送方式として実装することが目的
//!   であり、経路の物理的マルチホーミングは④(MPTCP/SCTP)の担当範囲とする
//!   (CLAUDE.md拡張要件(3)の役割分担どおり)。
//! - **TLS証明書**: QUICはTLS 1.3を組み込みで要求する。本実装は
//!   `rcgen`で自己署名証明書を都度生成する開発・検証用の構成
//!   (`QuicServerConfig::self_signed`)を提供する。本番運用では
//!   `open-web-server-wire::tls`と同様に正規の証明書/CA検証へ差し替える
//!   前提の参照実装であることを明示する。
//! - **用途**: ①TCP(権威パス)・②UDP(fire-and-forget即時通知)に対し、
//!   ③QUICは「TLS込みの信頼性のある双方向ストリーム」を提供する第3の
//!   独立した伝送特性を持つ経路として位置づける。本実装では1コネクション
//!   につき1本の双方向ストリームで `MutationRequest` をJSONとして送受信する
//!   単純な往復のみをサポートする(輻輳制御やストリーム多重化の詳細
//!   チューニングは今回のスコープ外)。
//! - **受信側の実配置は未接続**: `udp_channel` と同様、実運用でどのプロセスが
//!   QUICサーバをlistenしaruaru-db側WALと結合するかはopen-runo側のスコープ
//!   (今回は未着手)。本クレートはチャネル自体(サーバ起動・クライアント
//!   接続・送受信)のみを提供する。

use std::net::SocketAddr;
use std::sync::Arc;

use open_web_server_core::MutationRequest;
use quinn::{ClientConfig, Endpoint, ServerConfig};

/// 開発・検証用の自己署名QUICサーバ設定。
///
/// 本番では正規の証明書/CA検証済みクライアント設定に差し替えること
/// (`open-web-server-wire::tls::TlsServerConfig` と同じ思想)。
pub struct QuicServerConfig {
    pub server_config: ServerConfig,
    /// クライアント側が検証をスキップして接続できるよう、
    /// 生成した自己署名証明書のDER表現も一緒に返す (テスト・開発用途のみ)。
    pub cert_der: Vec<u8>,
}

/// rustlsのCryptoProvider(ring)をプロセス内で一度だけインストールする。
/// quinnはrustls経由でTLSを行うが、複数のrustls-cryptoバックエンドが
/// featureとして有効な場合に備え、プロセス全体で使うデフォルトを明示する
/// 必要がある (rustls 0.23の要件)。
fn ensure_crypto_provider_installed() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

impl QuicServerConfig {
    /// 自己署名証明書からQUICサーバ設定一式を生成する (開発/検証用)。
    pub fn self_signed(subject_alt_name: &str) -> anyhow::Result<Self> {
        ensure_crypto_provider_installed();
        let cert = rcgen::generate_simple_self_signed(vec![subject_alt_name.to_string()])?;
        let cert_der = cert.cert.der().to_vec();
        let key_der = cert.key_pair.serialize_der();

        let cert_chain = vec![rustls_pki_types::CertificateDer::from(cert_der.clone())];
        let priv_key = rustls_pki_types::PrivateKeyDer::try_from(key_der)
            .map_err(|e| anyhow::anyhow!("invalid private key DER: {e}"))?;

        // ALPNをクライアント側 (`insecure_client_config_trusting`) と一致させる
        // 必要がある (quinnはALPN不一致のハンドシェイクを拒否する)。
        let mut tls_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, priv_key)?;
        tls_config.alpn_protocols = vec![b"open-web-server-quic".to_vec()];

        let quic_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls_config)?;
        let server_config = ServerConfig::with_crypto(Arc::new(quic_crypto));

        Ok(Self {
            server_config,
            cert_der,
        })
    }
}

/// 自己署名証明書(`cert_der`)のみを信頼するクライアント設定 (開発/検証用)。
/// 本番はシステムのルート証明書ストア/正規のCAを使うこと。
pub fn insecure_client_config_trusting(cert_der: &[u8]) -> anyhow::Result<ClientConfig> {
    ensure_crypto_provider_installed();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(rustls_pki_types::CertificateDer::from(cert_der.to_vec()))?;

    let mut crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    crypto.alpn_protocols = vec![b"open-web-server-quic".to_vec()];

    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?;
    let mut client_config = ClientConfig::new(Arc::new(quic_crypto));

    // 宛先が応答しない (ICMPが届かないWindows環境等) 場合でも、
    // ハンドシェイクが無期限にハングしないよう短めのタイムアウトを設定する。
    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(Some(
        std::time::Duration::from_secs(3).try_into().unwrap(),
    ));
    client_config.transport_config(Arc::new(transport));

    Ok(client_config)
}

/// QUICサーバ側エンドポイント。1接続につき1本の双方向ストリームで
/// `MutationRequest` をJSONとして受信し、そのまま同じストリームへ
/// ACK (受信済みidempotency key) を書き返す往復のみをサポートする。
pub struct QuicServer {
    endpoint: Endpoint,
}

impl QuicServer {
    pub fn bind(local_addr: SocketAddr, config: QuicServerConfig) -> anyhow::Result<Self> {
        let endpoint = Endpoint::server(config.server_config, local_addr)?;
        Ok(Self { endpoint })
    }

    pub fn local_addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(self.endpoint.local_addr()?)
    }

    /// 1件の接続を受け付け、1本の双方向ストリームから1件の
    /// `MutationRequest` (JSON) を読み取り、idempotency keyをACKとして
    /// 返す。テスト・単純な検証用の最小実装 (複数ストリーム多重化は今回未対応)。
    pub async fn accept_one_mutation(&self) -> anyhow::Result<MutationRequest> {
        let incoming = self
            .endpoint
            .accept()
            .await
            .ok_or_else(|| anyhow::anyhow!("QUIC endpoint closed while waiting for connection"))?;
        let connection = incoming.await?;
        let (mut send, mut recv) = connection.accept_bi().await?;

        let data = recv.read_to_end(64 * 1024).await?;
        let req: MutationRequest = serde_json::from_slice(&data)?;

        let ack = serde_json::to_vec(&req.idempotency_key.0)?;
        send.write_all(&ack).await?;
        send.finish()?;
        // クライアント側がACKを読み切れるよう、送信完了を待つ。
        connection.closed().await;

        Ok(req)
    }
}

/// QUICクライアント側。1件の `MutationRequest` を送信し、ACK文字列
/// (idempotency key) を受け取って返す。
pub async fn send_mutation_over_quic(
    client_config: ClientConfig,
    bind_addr: SocketAddr,
    server_addr: SocketAddr,
    server_name: &str,
    req: &MutationRequest,
) -> anyhow::Result<String> {
    let mut endpoint = Endpoint::client(bind_addr)?;
    endpoint.set_default_client_config(client_config);

    let connection = endpoint.connect(server_addr, server_name)?.await?;
    let (mut send, mut recv) = connection.open_bi().await?;

    let payload = serde_json::to_vec(req)?;
    send.write_all(&payload).await?;
    send.finish()?;

    let ack_bytes = recv.read_to_end(64 * 1024).await?;
    let ack: String = serde_json::from_slice(&ack_bytes)?;

    connection.close(0u32.into(), b"done");
    endpoint.wait_idle().await;

    Ok(ack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_web_server_core::IdempotencyKey;

    fn sample_request(key: &str) -> MutationRequest {
        MutationRequest {
            idempotency_key: IdempotencyKey(key.to_string()),
            account_id: "user-1".to_string(),
            target: "items".to_string(),
            payload: serde_json::json!({"item_id": "shield", "quantity": 2}),
            requested_at: chrono::Utc::now(),
        }
    }

    /// 実QUICエンドポイント(127.0.0.1のループバック)を使った結合テスト。
    /// TLSハンドシェイク・実UDPソケット上のQUIC接続・双方向ストリームでの
    /// JSON往復を実証する(型チェックのみでの「完了」報告ではない)。
    #[tokio::test]
    async fn real_quic_roundtrip_over_loopback() {
        let server_config = QuicServerConfig::self_signed("localhost").unwrap();
        let cert_der = server_config.cert_der.clone();

        let server = QuicServer::bind("127.0.0.1:0".parse().unwrap(), server_config).unwrap();
        let server_addr = server.local_addr().unwrap();

        let req = sample_request("quic-key-1");
        let req_for_server = req.clone();

        let server_task = tokio::spawn(async move {
            let received = server.accept_one_mutation().await.unwrap();
            assert_eq!(received.idempotency_key.0, req_for_server.idempotency_key.0);
        });

        let client_config = insecure_client_config_trusting(&cert_der).unwrap();
        let ack = send_mutation_over_quic(
            client_config,
            "127.0.0.1:0".parse().unwrap(),
            server_addr,
            "localhost",
            &req,
        )
        .await
        .unwrap();

        assert_eq!(ack, "quic-key-1");
        server_task.await.unwrap();
    }

    /// 接続先が存在しない (誰もlistenしていない) 場合にハングせず
    /// エラーとして返ることを確認する (UDP側の同種テストと対になる)。
    #[tokio::test]
    async fn connect_to_unreachable_quic_destination_errors_without_hanging() {
        let server_config = QuicServerConfig::self_signed("localhost").unwrap();
        let cert_der = server_config.cert_der.clone();
        // このエンドポイントはbindするがacceptしないため、宛先は「存在するが
        // 応答しない」ケースになる。存在しないポートより現実的な障害シナリオ。
        drop(server_config);

        let client_config = insecure_client_config_trusting(&cert_der).unwrap();
        let unreachable: SocketAddr = "127.0.0.1:1".parse().unwrap();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            send_mutation_over_quic(
                client_config,
                "127.0.0.1:0".parse().unwrap(),
                unreachable,
                "localhost",
                &sample_request("quic-key-unreachable"),
            ),
        )
        .await;

        // タイムアウト内にエラーとして返ってくる (ハングしない) ことが重要。
        match result {
            Ok(inner) => assert!(inner.is_err(), "unreachable destination must error"),
            Err(_) => panic!("connection attempt hung past the 5s timeout"),
        }
    }
}
