//! ACME (RFC 8555) — HTTP-01チャレンジレスポンダ(常時コンパイル)+
//! ACMEクライアント本体(`acme` feature、2026-07-17に追加移植)。
//!
//! - **`ChallengeStore`+`challenge_response_handler`(常時コンパイル)**:
//!   ACME CA(Let's Encrypt等)が公開インターネット経由で実際に接続して
//!   くる`GET /.well-known/acme-challenge/:token`を提供する側。暗号処理・
//!   HTTPクライアント依存は一切無い。
//! - **`client`モジュール(`acme` feature)**: ディレクトリ探索・nonce
//!   管理・JWS署名・account/order/challenge/finalizeステートマシンを
//!   持つ、HTTP-01専用の最小ACME v2クライアント。`poem-cosmo-tauri`の
//!   `crates/open-runo-router/src/acme.rs`(`#[cfg(feature = "acme")]
//!   mod client`)を、`open_runo_core::{AppError, Result}`→
//!   `anyhow::Result`という型の違いだけを機械的に置き換えて移植した
//!   (`crate::hyper_compat::{Handler, Params}`への依存はこの部分の
//!   ロジックには元々存在しなかったため、それ以外は無変更)。JWS/JWK/
//!   base64url/CSR構築のロジック自体はpoem-cosmo-tauri側で既にモック
//!   ACME CA相手に実TCPで検証済みのコードと同一。

use std::collections::HashMap;
use std::sync::Mutex;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};

use crate::response::BoxBody;

/// トークン → key-authorization のインメモリ対応表。
/// `open-web-server`自体が公開ACMEクライアントを実装していなくても、
/// 外部のACMEクライアント(certbot等)がこのプロセスに向けて発行した
/// チャレンジをそのまま配信できるよう、常時コンパイルする(暗号/HTTP
/// クライアント依存が無いため軽量)。
#[derive(Debug, Default)]
pub struct ChallengeStore {
    tokens: Mutex<HashMap<String, String>>,
}

impl ChallengeStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&self, token: String, key_authorization: String) {
        self.tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(token, key_authorization);
    }

    pub fn get(&self, token: &str) -> Option<String> {
        self.tokens.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(token).cloned()
    }

    pub fn remove(&self, token: &str) {
        self.tokens.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(token);
    }
}

/// `GET /.well-known/acme-challenge/:token` — ACME CAのHTTP-01検証が
/// 実際に接続してくるエンドポイント。公開済みのkey authorizationを
/// `text/plain`で返す(RFC 8555 §8.3)。未公開/期限切れ/既に削除済みの
/// トークンは404。
pub async fn challenge_response_handler(store: &ChallengeStore, req: &Request<Incoming>) -> Response<BoxBody> {
    let path = req.uri().path();
    let Some(token) = path.strip_prefix("/.well-known/acme-challenge/") else {
        return not_found();
    };
    if token.is_empty() {
        return not_found();
    }
    match store.get(token) {
        Some(key_auth) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain")
            .body(BoxBody::from(key_auth))
            .expect("building a response from a fixed set of valid headers cannot fail"),
        None => not_found(),
    }
}

fn not_found() -> Response<BoxBody> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(BoxBody::from(String::new()))
        .expect("building a response from a fixed set of valid headers cannot fail")
}

#[cfg(feature = "acme")]
pub use client::*;

#[cfg(feature = "acme")]
mod client {
    use super::ChallengeStore;
    use anyhow::{anyhow, Result};
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_FIXED_SIGNING};
    use serde::Deserialize;
    use std::sync::Arc;

    /// Base64url, unpadded (RFC 7515 §2 / RFC 4648 §5) -- hand-rolled
    /// rather than adding a `base64` crate dependency, matching the same
    /// choice made in poem-cosmo-tauri's original implementation.
    fn base64url_encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0];
            let b1 = chunk.get(1).copied();
            let b2 = chunk.get(2).copied();
            out.push(ALPHABET[(b0 >> 2) as usize] as char);
            out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1.unwrap_or(0) >> 4)) as usize] as char);
            if let Some(b1) = b1 {
                out.push(ALPHABET[(((b1 & 0x0f) << 2) | (b2.unwrap_or(0) >> 6)) as usize] as char);
            }
            if let Some(b2) = b2 {
                out.push(ALPHABET[(b2 & 0x3f) as usize] as char);
            }
        }
        out
    }

    /// An ACME account's ES256 (ECDSA P-256 + SHA-256) key pair. Every ACME
    /// request is a JWS signed with this key.
    pub struct AcmeAccountKey {
        key_pair: EcdsaKeyPair,
        rng: SystemRandom,
    }

    impl AcmeAccountKey {
        pub fn generate() -> Result<Self> {
            let rng = SystemRandom::new();
            let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
                .map_err(|e| anyhow!("ACME account key generation failed: {e}"))?;
            let key_pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
                .map_err(|e| anyhow!("ACME account key parse failed: {e}"))?;
            Ok(Self { key_pair, rng })
        }

        /// Raw fixed-length (r||s) ES256 signature over `message`, per
        /// RFC 7518 §3.4.
        fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
            self.key_pair
                .sign(&self.rng, message)
                .map(|sig| sig.as_ref().to_vec())
                .map_err(|e| anyhow!("ACME JWS signing failed: {e}"))
        }

        /// The public key as a JWK (RFC 7517).
        fn jwk(&self) -> serde_json::Value {
            let public = self.key_pair.public_key().as_ref();
            debug_assert_eq!(public.len(), 65, "uncompressed P-256 point is 1+32+32 bytes");
            let x = &public[1..33];
            let y = &public[33..65];
            serde_json::json!({
                "kty": "EC",
                "crv": "P-256",
                "x": base64url_encode(x),
                "y": base64url_encode(y),
            })
        }

        /// RFC 7638 JWK thumbprint: base64url(SHA-256(canonical JSON)).
        pub fn thumbprint(&self) -> String {
            let public = self.key_pair.public_key().as_ref();
            let x = base64url_encode(&public[1..33]);
            let y = base64url_encode(&public[33..65]);
            let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
            let digest = ring::digest::digest(&ring::digest::SHA256, canonical.as_bytes());
            base64url_encode(digest.as_ref())
        }
    }

    /// `key-authorization` for an HTTP-01 challenge (RFC 8555 §8.1).
    pub fn http01_key_authorization(token: &str, account_key: &AcmeAccountKey) -> String {
        format!("{token}.{}", account_key.thumbprint())
    }

    #[derive(Debug, Clone, Deserialize)]
    struct AcmeDirectory {
        #[serde(rename = "newNonce")]
        new_nonce: String,
        #[serde(rename = "newAccount")]
        new_account: String,
        #[serde(rename = "newOrder")]
        new_order: String,
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct AcmeOrder {
        pub status: String,
        pub authorizations: Vec<String>,
        pub finalize: String,
        pub certificate: Option<String>,
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct AcmeChallenge {
        #[serde(rename = "type")]
        pub challenge_type: String,
        pub url: String,
        pub token: String,
        pub status: String,
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct AcmeAuthorization {
        pub status: String,
        pub challenges: Vec<AcmeChallenge>,
    }

    /// A minimal ACME v2 client: enough of RFC 8555 to obtain a certificate
    /// via a single HTTP-01 challenge. Not a general-purpose ACME library --
    /// no DNS-01/TLS-ALPN-01, no account key rollover, no revocation.
    pub struct AcmeClient {
        http: reqwest::Client,
        directory: AcmeDirectory,
        account_key: AcmeAccountKey,
        kid: Option<String>,
        nonce: Option<String>,
    }

    impl AcmeClient {
        pub async fn discover(directory_url: &str) -> Result<Self> {
            let http = reqwest::Client::new();
            let directory: AcmeDirectory = http
                .get(directory_url)
                .send()
                .await
                .map_err(|e| anyhow!("ACME directory fetch failed: {e}"))?
                .json()
                .await
                .map_err(|e| anyhow!("ACME directory parse failed: {e}"))?;
            Ok(Self { http, directory, account_key: AcmeAccountKey::generate()?, kid: None, nonce: None })
        }

        async fn fetch_nonce(&self) -> Result<String> {
            let resp = self
                .http
                .head(&self.directory.new_nonce)
                .send()
                .await
                .map_err(|e| anyhow!("ACME newNonce failed: {e}"))?;
            resp.headers()
                .get("replay-nonce")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
                .ok_or_else(|| anyhow!("ACME newNonce response missing Replay-Nonce"))
        }

        async fn post_jws(&mut self, url: &str, payload: Option<serde_json::Value>) -> Result<(reqwest::header::HeaderMap, serde_json::Value)> {
            let nonce = match self.nonce.take() {
                Some(n) => n,
                None => self.fetch_nonce().await?,
            };

            let mut protected = serde_json::json!({ "alg": "ES256", "nonce": nonce, "url": url });
            match &self.kid {
                Some(kid) => protected["kid"] = serde_json::Value::String(kid.clone()),
                None => protected["jwk"] = self.account_key.jwk(),
            }

            let protected_b64 = base64url_encode(&serde_json::to_vec(&protected).unwrap());
            let payload_b64 = match &payload {
                Some(p) => base64url_encode(&serde_json::to_vec(p).unwrap()),
                None => String::new(),
            };
            let signing_input = format!("{protected_b64}.{payload_b64}");
            let signature = self.account_key.sign(signing_input.as_bytes())?;

            let body = serde_json::json!({
                "protected": protected_b64,
                "payload": payload_b64,
                "signature": base64url_encode(&signature),
            });

            let resp = self
                .http
                .post(url)
                .header("content-type", "application/jose+json")
                .json(&body)
                .send()
                .await
                .map_err(|e| anyhow!("ACME request to {url} failed: {e}"))?;

            if let Some(next_nonce) = resp.headers().get("replay-nonce").and_then(|v| v.to_str().ok()) {
                self.nonce = Some(next_nonce.to_string());
            }

            let status = resp.status();
            let headers = resp.headers().clone();
            let bytes = resp.bytes().await.map_err(|e| anyhow!("ACME response body read failed: {e}"))?;
            let parsed: serde_json::Value = if bytes.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::from_slice(&bytes).map_err(|e| anyhow!("ACME response JSON parse failed: {e}"))?
            };

            if !status.is_success() {
                return Err(anyhow!("ACME request to {url} returned {status}: {parsed}"));
            }

            Ok((headers, parsed))
        }

        pub async fn new_account(&mut self, contact_emails: &[String], terms_agreed: bool) -> Result<()> {
            let url = self.directory.new_account.clone();
            let contact: Vec<String> = contact_emails.iter().map(|e| format!("mailto:{e}")).collect();
            let payload = serde_json::json!({ "termsOfServiceAgreed": terms_agreed, "contact": contact });
            let (headers, _body) = self.post_jws(&url, Some(payload)).await?;
            let kid = headers
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| anyhow!("ACME newAccount response missing Location"))?
                .to_string();
            self.kid = Some(kid);
            Ok(())
        }

        pub async fn new_order(&mut self, domains: &[String]) -> Result<AcmeOrder> {
            let url = self.directory.new_order.clone();
            let identifiers: Vec<serde_json::Value> =
                domains.iter().map(|d| serde_json::json!({ "type": "dns", "value": d })).collect();
            let payload = serde_json::json!({ "identifiers": identifiers });
            let (_headers, body) = self.post_jws(&url, Some(payload)).await?;
            serde_json::from_value(body).map_err(|e| anyhow!("ACME order parse failed: {e}"))
        }

        pub async fn get_authorization(&mut self, url: &str) -> Result<AcmeAuthorization> {
            let (_headers, body) = self.post_jws(url, None).await?;
            serde_json::from_value(body).map_err(|e| anyhow!("ACME authorization parse failed: {e}"))
        }

        pub fn key_authorization_for(&self, token: &str) -> String {
            http01_key_authorization(token, &self.account_key)
        }

        pub async fn respond_to_challenge(&mut self, challenge_url: &str) -> Result<()> {
            self.post_jws(challenge_url, Some(serde_json::json!({}))).await?;
            Ok(())
        }

        pub async fn poll_authorization_until_valid(&mut self, authorization_url: &str, max_attempts: u32) -> Result<()> {
            for _ in 0..max_attempts {
                let auth = self.get_authorization(authorization_url).await?;
                match auth.status.as_str() {
                    "valid" => return Ok(()),
                    "pending" => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
                    other => return Err(anyhow!("ACME authorization {authorization_url} ended in status {other}")),
                }
            }
            Err(anyhow!("ACME authorization {authorization_url} still pending after {max_attempts} attempts"))
        }

        pub async fn finalize_order(&mut self, order: &AcmeOrder, domain: &str) -> Result<(AcmeOrder, String)> {
            let key_pair = rcgen::KeyPair::generate().map_err(|e| anyhow!("certificate key generation failed: {e}"))?;
            let params = rcgen::CertificateParams::new(vec![domain.to_string()])
                .map_err(|e| anyhow!("certificate params failed: {e}"))?;
            let csr = params.serialize_request(&key_pair).map_err(|e| anyhow!("CSR generation failed: {e}"))?;
            let key_pem = key_pair.serialize_pem();

            let payload = serde_json::json!({ "csr": base64url_encode(csr.der()) });
            let (_headers, body) = self.post_jws(&order.finalize, Some(payload)).await?;
            let finalized: AcmeOrder =
                serde_json::from_value(body).map_err(|e| anyhow!("ACME finalize response parse failed: {e}"))?;
            Ok((finalized, key_pem))
        }

        pub async fn download_certificate(&mut self, certificate_url: &str) -> Result<String> {
            let nonce = match self.nonce.take() {
                Some(n) => n,
                None => self.fetch_nonce().await?,
            };
            let mut protected = serde_json::json!({ "alg": "ES256", "nonce": nonce, "url": certificate_url });
            protected["kid"] =
                serde_json::Value::String(self.kid.clone().ok_or_else(|| anyhow!("no ACME account kid"))?);
            let protected_b64 = base64url_encode(&serde_json::to_vec(&protected).unwrap());
            let payload_b64 = String::new();
            let signing_input = format!("{protected_b64}.{payload_b64}");
            let signature = self.account_key.sign(signing_input.as_bytes())?;
            let body = serde_json::json!({
                "protected": protected_b64,
                "payload": payload_b64,
                "signature": base64url_encode(&signature),
            });

            let resp = self
                .http
                .post(certificate_url)
                .header("content-type", "application/jose+json")
                .json(&body)
                .send()
                .await
                .map_err(|e| anyhow!("ACME certificate download failed: {e}"))?;
            if let Some(next_nonce) = resp.headers().get("replay-nonce").and_then(|v| v.to_str().ok()) {
                self.nonce = Some(next_nonce.to_string());
            }
            resp.text().await.map_err(|e| anyhow!("ACME certificate body read failed: {e}"))
        }
    }

    /// End-to-end orchestration: discover → register → order → publish
    /// challenge response into `challenges` → respond → poll → finalize →
    /// download. Returns `(certificate_chain_pem, private_key_pem)`.
    pub async fn obtain_certificate_http01(
        directory_url: &str,
        domain: &str,
        contact_email: &str,
        challenges: &Arc<ChallengeStore>,
    ) -> Result<(String, String)> {
        let mut client = AcmeClient::discover(directory_url).await?;
        client.new_account(&[contact_email.to_string()], true).await?;
        let order = client.new_order(&[domain.to_string()]).await?;

        for auth_url in &order.authorizations {
            let auth = client.get_authorization(auth_url).await?;
            let challenge = auth
                .challenges
                .iter()
                .find(|c| c.challenge_type == "http-01")
                .ok_or_else(|| anyhow!("no http-01 challenge offered"))?;

            let key_auth = client.key_authorization_for(&challenge.token);
            challenges.publish(challenge.token.clone(), key_auth);
            client.respond_to_challenge(&challenge.url).await?;
            client.poll_authorization_until_valid(auth_url, 20).await?;
            challenges.remove(&challenge.token);
        }

        let (finalized, key_pem) = client.finalize_order(&order, domain).await?;
        let cert_url = finalized.certificate.ok_or_else(|| anyhow!("ACME order finalized without a certificate URL"))?;
        let cert_pem = client.download_certificate(&cert_url).await?;
        Ok((cert_pem, key_pem))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn base64url_has_no_padding_and_uses_url_safe_alphabet() {
            let encoded = base64url_encode(b"any carnal pleasure.");
            assert_eq!(encoded, "YW55IGNhcm5hbCBwbGVhc3VyZS4");
            assert!(!encoded.contains('='));
            assert!(!encoded.contains('+'));
            assert!(!encoded.contains('/'));
        }

        #[test]
        fn thumbprint_is_stable_for_the_same_key() {
            let key = AcmeAccountKey::generate().unwrap();
            assert_eq!(key.thumbprint(), key.thumbprint());
        }

        #[test]
        fn different_keys_have_different_thumbprints() {
            let a = AcmeAccountKey::generate().unwrap();
            let b = AcmeAccountKey::generate().unwrap();
            assert_ne!(a.thumbprint(), b.thumbprint());
        }

        #[test]
        fn http01_key_authorization_is_token_dot_thumbprint() {
            let key = AcmeAccountKey::generate().unwrap();
            let key_auth = http01_key_authorization("abc123", &key);
            assert_eq!(key_auth, format!("abc123.{}", key.thumbprint()));
        }

        #[tokio::test]
        async fn sign_produces_a_64_byte_fixed_signature() {
            let key = AcmeAccountKey::generate().unwrap();
            for _ in 0..5 {
                let sig = key.sign(b"test message").unwrap();
                assert_eq!(sig.len(), 64, "ES256 JWS signatures must be fixed-length r||s, not ASN.1 DER");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `challenge_response_handler`は`Request<Incoming>`(実TCP接続からしか
    /// 構築できない型)の`uri().path()`しか読まないため、パス抽出ロジック
    /// だけを切り出してユニットテストする(ハンドラ自体のエンドツーエンド
    /// 検証は実サーバー起動を伴う統合テストの領分——このモジュールの
    /// スコープは「正しいパスをパースするか」に絞る)。
    fn extract_token(path: &str) -> Option<&str> {
        let token = path.strip_prefix("/.well-known/acme-challenge/")?;
        if token.is_empty() {
            None
        } else {
            Some(token)
        }
    }

    #[test]
    fn extracts_token_from_well_known_path() {
        assert_eq!(extract_token("/.well-known/acme-challenge/abc123"), Some("abc123"));
    }

    #[test]
    fn rejects_empty_token_and_unrelated_paths() {
        assert_eq!(extract_token("/.well-known/acme-challenge/"), None);
        assert_eq!(extract_token("/healthz"), None);
    }

    #[test]
    fn publish_get_remove_round_trip() {
        let store = ChallengeStore::new();
        assert_eq!(store.get("token-a"), None);
        store.publish("token-a".to_string(), "key-auth-a".to_string());
        assert_eq!(store.get("token-a"), Some("key-auth-a".to_string()));
        store.remove("token-a");
        assert_eq!(store.get("token-a"), None);
        // Removing again (already absent) must not panic -- idempotent.
        store.remove("token-a");
    }

    /// エンドツーエンド検証: 本物の`challenge_response_handler`(実運用と
    /// 同一コード)+ 実TCP上のモックACME CA(JWS署名自体は検証しないが、
    /// HTTP-01のチャレンジ検証だけは本物のループバックHTTP GETで行う
    /// ——「サーバーが本当にkey authorizationを公開したか」を偽装なしで
    /// 確認する)を組み合わせ、`obtain_certificate_http01`が
    /// discover→account→order→challenge公開→検証→finalize→ダウンロード
    /// まで一気通貫で動くことを実証する。JWS署名の暗号的検証まではCA側で
    /// 行わない(それには本テストが検証したいクライアント側のロジックを
    /// サーバー側で再実装する必要があるため)——ここで検証しているのは
    /// ディレクトリ/nonce/account/order/authorization/challenge/finalize/
    /// downloadという形状とステートマシンであり、これは実CAと相互運用
    /// するために`AcmeClient`が正しく持つ必要がある部分。
    #[cfg(feature = "acme")]
    #[tokio::test]
    async fn full_http01_flow_against_mock_ca_with_real_challenge_loopback() {
        use bytes::Bytes;
        use http_body_util::Full;
        use hyper::server::conn::http1;
        use hyper::service::service_fn;
        use hyper::{Method, StatusCode};
        use hyper_util::rt::TokioIo;
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::{Arc, Mutex as StdMutex};
        use tokio::net::TcpListener;

        type MockBody = Full<Bytes>;

        fn json_resp(status: StatusCode, value: &serde_json::Value, extra_headers: &[(&str, String)]) -> Response<MockBody> {
            let mut builder = Response::builder().status(status).header("content-type", "application/json");
            for (name, value) in extra_headers {
                builder = builder.header(*name, value.as_str());
            }
            builder.body(MockBody::new(Bytes::from(value.to_string()))).unwrap()
        }

        const TOKEN: &str = "test-challenge-token";
        const FAKE_CERT_PEM: &str = "-----BEGIN CERTIFICATE-----\nMOCK\n-----END CERTIFICATE-----\n";

        // 1. The real, production challenge-response server.
        let challenge_store = Arc::new(ChallengeStore::new());
        let challenge_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let challenge_addr = challenge_listener.local_addr().unwrap();
        {
            let challenge_store = Arc::clone(&challenge_store);
            tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = challenge_listener.accept().await else { continue };
                    let io = TokioIo::new(stream);
                    let challenge_store = Arc::clone(&challenge_store);
                    tokio::spawn(async move {
                        let service = service_fn(move |req: Request<Incoming>| {
                            let challenge_store = Arc::clone(&challenge_store);
                            async move {
                                let resp = super::challenge_response_handler(&challenge_store, &req).await;
                                Ok::<_, std::convert::Infallible>(resp)
                            }
                        });
                        let _ = http1::Builder::new().serve_connection(io, service).await;
                    });
                }
            });
        }

        // 2. The mock CA. `ca_base` starts empty and is filled in once this
        // server itself is bound (route closures only read it at request
        // time, after it's populated).
        let ca_base: Arc<StdMutex<String>> = Arc::new(StdMutex::new(String::new()));
        let nonce_counter = Arc::new(AtomicU64::new(0));
        let challenge_validated = Arc::new(AtomicBool::new(false));
        let finalized = Arc::new(AtomicBool::new(false));

        let ca_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ca_addr = ca_listener.local_addr().unwrap();
        *ca_base.lock().unwrap() = format!("http://{ca_addr}");

        {
            let ca_base = Arc::clone(&ca_base);
            let nonce_counter = Arc::clone(&nonce_counter);
            let challenge_validated = Arc::clone(&challenge_validated);
            let finalized = Arc::clone(&finalized);
            tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = ca_listener.accept().await else { continue };
                    let io = TokioIo::new(stream);
                    let ca_base = Arc::clone(&ca_base);
                    let nonce_counter = Arc::clone(&nonce_counter);
                    let challenge_validated = Arc::clone(&challenge_validated);
                    let finalized = Arc::clone(&finalized);
                    tokio::spawn(async move {
                        let service = service_fn(move |req: Request<Incoming>| {
                            let base = ca_base.lock().unwrap().clone();
                            let nonce = format!("nonce-{}", nonce_counter.fetch_add(1, Ordering::SeqCst));
                            let challenge_validated = Arc::clone(&challenge_validated);
                            let finalized = Arc::clone(&finalized);
                            async move {
                                let path = req.uri().path().to_string();
                                let method = req.method().clone();
                                let resp = match (&method, path.as_str()) {
                                    (&Method::GET, "/directory") => json_resp(
                                        StatusCode::OK,
                                        &serde_json::json!({
                                            "newNonce": format!("{base}/new-nonce"),
                                            "newAccount": format!("{base}/new-acct"),
                                            "newOrder": format!("{base}/new-order"),
                                        }),
                                        &[],
                                    ),
                                    (&Method::HEAD, "/new-nonce") => {
                                        Response::builder().status(StatusCode::OK).header("replay-nonce", nonce).body(MockBody::new(Bytes::new())).unwrap()
                                    }
                                    (&Method::POST, "/new-acct") => json_resp(
                                        StatusCode::CREATED,
                                        &serde_json::json!({ "status": "valid" }),
                                        &[("location", format!("{base}/acct/1")), ("replay-nonce", nonce)],
                                    ),
                                    (&Method::POST, "/new-order") => json_resp(
                                        StatusCode::CREATED,
                                        &serde_json::json!({
                                            "status": "pending",
                                            "authorizations": [format!("{base}/authz/1")],
                                            "finalize": format!("{base}/finalize/1"),
                                        }),
                                        &[("location", format!("{base}/order/1")), ("replay-nonce", nonce)],
                                    ),
                                    (&Method::POST, "/authz/1") => {
                                        let status = if challenge_validated.load(Ordering::SeqCst) { "valid" } else { "pending" };
                                        json_resp(
                                            StatusCode::OK,
                                            &serde_json::json!({
                                                "status": status,
                                                "challenges": [{
                                                    "type": "http-01",
                                                    "url": format!("{base}/challenge/1"),
                                                    "token": TOKEN,
                                                    "status": status,
                                                }],
                                            }),
                                            &[("replay-nonce", nonce)],
                                        )
                                    }
                                    (&Method::POST, "/challenge/1") => {
                                        // The real validation step: fetch the token from the
                                        // *actual* challenge-response server, over real HTTP.
                                        let url = format!("http://{challenge_addr}/.well-known/acme-challenge/{TOKEN}");
                                        if let Ok(resp) = reqwest::get(&url).await {
                                            if resp.status().is_success() {
                                                if let Ok(body) = resp.text().await {
                                                    if !body.is_empty() {
                                                        challenge_validated.store(true, Ordering::SeqCst);
                                                    }
                                                }
                                            }
                                        }
                                        json_resp(StatusCode::OK, &serde_json::json!({ "status": "processing" }), &[("replay-nonce", nonce)])
                                    }
                                    (&Method::POST, "/finalize/1") => {
                                        finalized.store(true, Ordering::SeqCst);
                                        json_resp(
                                            StatusCode::OK,
                                            &serde_json::json!({
                                                "status": "valid",
                                                "authorizations": [format!("{base}/authz/1")],
                                                "finalize": format!("{base}/finalize/1"),
                                                "certificate": format!("{base}/cert/1"),
                                            }),
                                            &[("replay-nonce", nonce)],
                                        )
                                    }
                                    (&Method::POST, "/cert/1") => Response::builder()
                                        .status(StatusCode::OK)
                                        .header("content-type", "application/pem-certificate-chain")
                                        .body(MockBody::new(Bytes::from_static(FAKE_CERT_PEM.as_bytes())))
                                        .unwrap(),
                                    _ => Response::builder().status(StatusCode::NOT_FOUND).body(MockBody::new(Bytes::new())).unwrap(),
                                };
                                Ok::<_, std::convert::Infallible>(resp)
                            }
                        });
                        let _ = http1::Builder::new().serve_connection(io, service).await;
                    });
                }
            });
        }

        // 3. Run the real client against the mock CA end to end.
        let directory_url = format!("http://{ca_addr}/directory");
        let (cert_pem, key_pem) = obtain_certificate_http01(&directory_url, "test.local", "admin@test.local", &challenge_store)
            .await
            .expect("full ACME HTTP-01 flow should succeed against the mock CA");

        assert_eq!(cert_pem, FAKE_CERT_PEM);
        assert!(key_pem.contains("PRIVATE KEY"), "should return a real PEM private key");
        assert!(
            challenge_validated.load(Ordering::SeqCst),
            "the mock CA's loopback fetch must have actually observed a published key authorization"
        );
        assert!(finalized.load(Ordering::SeqCst));
        assert!(challenge_store.get(TOKEN).is_none(), "challenge token should be removed after use");
    }
}
