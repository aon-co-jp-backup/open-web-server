//! 第1層: 伝送路暗号化 (TLS 1.3 / rustls)

use std::{collections::HashMap, fs::File, io::BufReader, path::Path, sync::Arc, sync::RwLock};

use rustls::sign::CertifiedKey;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

#[derive(Debug, Clone)]
pub struct TlsServerConfig {
    pub cert_path: String,
    pub key_path: String,
}

impl TlsServerConfig {
    pub fn load(&self) -> anyhow::Result<Arc<rustls::ServerConfig>> {
        let certs = load_certs(&self.cert_path)?;
        let key = load_key(&self.key_path)?;

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        Ok(Arc::new(config))
    }
}

fn load_certs(path: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let file = File::open(Path::new(path))?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    Ok(certs)
}

fn load_key(path: &str) -> anyhow::Result<PrivateKeyDer<'static>> {
    let file = File::open(Path::new(path))?;
    let mut reader = BufReader::new(file);
    let key = rustls_pemfile::private_key(&mut reader)?
        .ok_or_else(|| anyhow::anyhow!("no private key found at {path}"))?;
    Ok(key)
}

fn parse_cert_chain(pem: &[u8]) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(pem);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        anyhow::bail!("no certificates found in PEM input");
    }
    Ok(certs)
}

fn parse_private_key(pem: &[u8]) -> anyhow::Result<PrivateKeyDer<'static>> {
    let mut reader = BufReader::new(pem);
    rustls_pemfile::private_key(&mut reader)?.ok_or_else(|| anyhow::anyhow!("no private key found in PEM input"))
}

/// rustlsのCryptoProvider(ring)をプロセス内で一度だけインストールする
/// (`quic_channel.rs`の同名ヘルパーと同じ理由・同じ実装 — rustls 0.23は
/// 複数のcrypto backendがfeatureとして有効な場合に備え、プロセス全体で
/// 使うデフォルトを明示する必要がある)。`ServerConfig::builder()`/
/// `ClientConfig::builder()`(引数無し版)はこれが未インストールだと
/// パニックするため、これらを呼ぶ前に必ず呼び出すこと。
fn ensure_crypto_provider_installed() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// SNI(ClientHelloのserver_name)に応じて、テナント(ドメイン)ごとに別々の
/// 証明書を返す `ResolvesServerCert` 実装。これが無いと、open-web-server
/// 自体は1プロセスにつき1証明書しか提供できず(既存の`TlsServerConfig`)、
/// `tenant_router::TenantRegistry`が既に実現している「1プロセスで複数
/// ドメインを動的に振り分ける」というマルチテナント運用を、TLS終端の面
/// では実現できていなかった——本リゾルバがその欠落を埋める。
///
/// 実世界の同種実装(rustls上で複数ドメインをTLS終端するリバースプロキシ
/// `rpxy`等)と同じ、`rustls::server::ResolvesServerCert` + ホスト名ごとの
/// `CertifiedKey`辞書という標準パターンに沿う(2026-07-16、EN/JP両言語で
/// 実務例を調査済み)。
#[derive(Debug, Default)]
pub struct TenantCertResolver {
    certs: RwLock<HashMap<String, Arc<CertifiedKey>>>,
}

impl TenantCertResolver {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// `host`(SNI名、大文字小文字は無視)にPEM形式の証明書チェーン+秘密鍵を
    /// 登録する。既存の登録は上書きされる(証明書更新・ACME再発行後の
    /// ローテーションに使う)。
    pub fn upsert_pem(&self, host: &str, cert_chain_pem: &[u8], key_pem: &[u8]) -> anyhow::Result<()> {
        let chain = parse_cert_chain(cert_chain_pem)?;
        let key = parse_private_key(key_pem)?;
        let signing_key = rustls::crypto::ring::sign::any_supported_type(&key)
            .map_err(|e| anyhow::anyhow!("unsupported private key for {host}: {e}"))?;
        let certified_key = Arc::new(CertifiedKey::new(chain, signing_key));
        self.certs
            .write()
            .map_err(|_| anyhow::anyhow!("TenantCertResolver lock poisoned"))?
            .insert(host.to_ascii_lowercase(), certified_key);
        Ok(())
    }

    /// `host`(PEMファイルパス版)。ACME自動更新やvhost追加時、ディスク上の
    /// 証明書ファイルからそのまま登録したい場合の薄いラッパー。
    pub fn upsert_from_files(&self, host: &str, cert_path: &str, key_path: &str) -> anyhow::Result<()> {
        let cert_pem = std::fs::read(cert_path)?;
        let key_pem = std::fs::read(key_path)?;
        self.upsert_pem(host, &cert_pem, &key_pem)
    }

    /// `host`の証明書登録を削除する(テナント削除時、Apacheの`a2dissite`
    /// 相当)。登録が無かった場合も静かに成功する(冪等)。
    pub fn remove(&self, host: &str) -> anyhow::Result<()> {
        self.certs
            .write()
            .map_err(|_| anyhow::anyhow!("TenantCertResolver lock poisoned"))?
            .remove(&host.to_ascii_lowercase());
        Ok(())
    }

    pub fn contains(&self, host: &str) -> bool {
        self.certs
            .read()
            .map(|map| map.contains_key(&host.to_ascii_lowercase()))
            .unwrap_or(false)
    }
}

impl rustls::server::ResolvesServerCert for TenantCertResolver {
    fn resolve(&self, client_hello: rustls::server::ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let server_name = client_hello.server_name()?;
        self.certs.read().ok()?.get(&server_name.to_ascii_lowercase()).cloned()
    }
}

/// `TenantCertResolver`をSNIに応じた証明書選択に使う`ServerConfig`を組み立てる。
/// クライアント証明書認証は行わない(このアプリの認証はHTTP層のAPIキー/
/// テナント振り分けであり、TLS層のmTLSは既存の`open-web-server-wire`の
/// バックエンド間4層防御通信の方に別途ある——ここは公開向けの通常TLS)。
pub fn build_tenant_server_config(resolver: Arc<TenantCertResolver>) -> Arc<rustls::ServerConfig> {
    ensure_crypto_provider_installed();
    Arc::new(rustls::ServerConfig::builder().with_no_client_auth().with_cert_resolver(resolver))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `rcgen`で使い捨ての自己署名証明書(PEM)を1組生成する。ディスクへの
    /// 書き込みは行わない(このテストは`upsert_pem`のインメモリ経路のみを
    /// 検証する——`upsert_from_files`はこの関数の薄いラッパーなので
    /// 別途のファイルI/Oテストは不要と判断)。
    fn self_signed_pem(subject_alt_name: &str) -> (Vec<u8>, Vec<u8>) {
        let cert = rcgen::generate_simple_self_signed(vec![subject_alt_name.to_string()]).unwrap();
        (cert.cert.pem().into_bytes(), cert.key_pair.serialize_pem().into_bytes())
    }

    #[test]
    fn upsert_then_resolve_returns_none_for_unknown_host() {
        let resolver = TenantCertResolver::new();
        let (cert_pem, key_pem) = self_signed_pem("tenant-a.example.test");
        resolver.upsert_pem("tenant-a.example.test", &cert_pem, &key_pem).unwrap();

        assert!(resolver.contains("tenant-a.example.test"));
        assert!(!resolver.contains("unknown-host.example.test"));
    }

    #[test]
    fn upsert_is_case_insensitive_and_remove_is_idempotent() {
        let resolver = TenantCertResolver::new();
        let (cert_pem, key_pem) = self_signed_pem("Tenant-B.example.test");
        resolver.upsert_pem("Tenant-B.example.test", &cert_pem, &key_pem).unwrap();

        assert!(resolver.contains("tenant-b.example.test"));
        resolver.remove("TENANT-B.example.test").unwrap();
        assert!(!resolver.contains("tenant-b.example.test"));
        // Removing again (already absent) must not error -- idempotent, like
        // the existing `tenant_router::remove` semantics this mirrors.
        resolver.remove("tenant-b.example.test").unwrap();
    }

    #[test]
    fn upsert_rejects_malformed_pem() {
        let resolver = TenantCertResolver::new();
        assert!(resolver.upsert_pem("bad.example.test", b"not a certificate", b"not a key").is_err());
    }

    /// これが本テストモジュールの核心: 同一プロセス/同一`ServerConfig`が、
    /// SNIサーバー名だけを見て2つの異なるテナントに別々の証明書を実際に
    /// 返すことを、本物のTLSハンドシェイク(実TCPループバック)で証明する。
    /// 単体テストレベルの「辞書に入っているか」の確認(上記2件)だけでは、
    /// `ResolvesServerCert`の実装がrustls自体から正しく呼ばれる配線に
    /// なっているかまでは検証できないため、これを別途実施する。
    // Test-only verifier: records whatever leaf certificate the server
    // presented and accepts it unconditionally. Production code never uses
    // this -- it exists solely so this test can complete a real TLS 1.3
    // handshake against a self-signed cert without a trust anchor, while
    // still letting the test assert on which cert bytes came back.
    #[derive(Debug, Default)]
    struct RecordingVerifier {
        leaf_der: std::sync::Mutex<Option<Vec<u8>>>,
    }
    impl rustls::client::danger::ServerCertVerifier for RecordingVerifier {
        fn verify_server_cert(
            &self,
            end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            *self.leaf_der.lock().unwrap() = Some(end_entity.to_vec());
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
        }
    }

    async fn handshake_and_get_leaf_cert(
        listener: tokio::net::TcpListener,
        acceptor: tokio_rustls::TlsAcceptor,
        sni: &'static str,
    ) -> Vec<u8> {
        use rustls::pki_types::ServerName;
        use tokio::net::TcpStream;
        use tokio_rustls::TlsConnector;

        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = acceptor.accept(stream).await.unwrap();
        });

        ensure_crypto_provider_installed();
        let verifier = Arc::new(RecordingVerifier::default());
        let client_config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(client_config));

        let tcp = TcpStream::connect(addr).await.unwrap();
        let server_name = ServerName::try_from(sni).unwrap();
        let _tls_stream = connector.connect(server_name, tcp).await.unwrap();
        server_task.await.unwrap();

        let leaf = verifier.leaf_der.lock().unwrap().clone().unwrap();
        leaf
    }

    fn first_cert_der(pem: &[u8]) -> Vec<u8> {
        let mut reader = BufReader::new(pem);
        let der = rustls_pemfile::certs(&mut reader).next().unwrap().unwrap().to_vec();
        der
    }

    /// これが本テストモジュールの核心: 同一プロセス/同一`ServerConfig`が、
    /// SNIサーバー名だけを見て2つの異なるテナントに別々の証明書を実際に
    /// 返すことを、本物のTLSハンドシェイク(実TCPループバック)で証明する。
    /// 単体テストレベルの「辞書に入っているか」の確認(上記2件)だけでは、
    /// `ResolvesServerCert`の実装がrustls自体から正しく呼ばれる配線に
    /// なっているかまでは検証できないため、これを別途実施する。
    #[tokio::test]
    async fn real_tls_handshake_resolves_different_cert_per_sni() {
        use tokio::net::TcpListener;
        use tokio_rustls::TlsAcceptor;

        let resolver = TenantCertResolver::new();
        let (cert_a_pem, key_a_pem) = self_signed_pem("tenant-a.example.test");
        let (cert_b_pem, key_b_pem) = self_signed_pem("tenant-b.example.test");
        resolver.upsert_pem("tenant-a.example.test", &cert_a_pem, &key_a_pem).unwrap();
        resolver.upsert_pem("tenant-b.example.test", &cert_b_pem, &key_b_pem).unwrap();

        let server_config = build_tenant_server_config(resolver);
        let acceptor = TlsAcceptor::from(server_config);

        // Each handshake gets its own freshly-bound ephemeral-port listener
        // (rather than reusing one address across two sequential accepts),
        // so there's no risk of a port-reuse race between the two handshakes.
        let listener_a = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let leaf_a = handshake_and_get_leaf_cert(listener_a, acceptor.clone(), "tenant-a.example.test").await;

        let listener_b = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let leaf_b = handshake_and_get_leaf_cert(listener_b, acceptor, "tenant-b.example.test").await;

        assert_ne!(leaf_a, leaf_b, "different SNI names must resolve to different certificates");
        assert_eq!(leaf_a, first_cert_der(&cert_a_pem));
        assert_eq!(leaf_b, first_cert_der(&cert_b_pem));
    }
}
