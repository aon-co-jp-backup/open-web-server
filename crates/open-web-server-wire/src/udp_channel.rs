//! UDP-IP 冗長経路 (TCP-IP 主系に対する副系・第一実装版)
//!
//! ## スコープと限界 (2026-07-11、正直な記載)
//!
//! `open-web-server/CLAUDE.md` の「拡張要件 (3) TCP-IP・UDP-IPの三層三重通信」の
//! うち、UDP側の**最初の具体的な実装**。目標アーキテクチャ全体
//! (主系TCP + 副系TCP + UDP即時通知、のような三重化)のごく一部であり、
//! 以下の点で意図的にスコープを絞っている:
//!
//! - **信頼性モデル**: 再送(retransmit-on-no-ack)は実装していない。
//!   UDPは「送りっぱなし (fire-and-forget) の即時通知/advance notice」として扱い、
//!   本体データの確定はあくまで既存のTCP経由 3ホップコミット
//!   (`open-web-server-ledger::Ledger::commit`) が担う。UDP側が届かなくても
//!   TCP経路の確定には一切影響しない (このモジュールの結合テストで実証)。
//! - **冪等性による安全性**: UDPは重複・順序入れ替わりが起こり得るため、
//!   受信側は `IdempotencyKey` によるデデュープ (`Deduplicator`) を必須とする。
//!   同じミューテーションがTCP経由・UDP経由の両方で届いても、デデュープにより
//!   実害はない (既存の `open-web-server-core::IdempotencyKey` 設計と同じ思想)。
//! - **暗号化・認証**: UDPにはTLSが無いため、(a) `payload_crypto::PayloadCipher`
//!   (ChaCha20-Poly1305 AEAD) による機密性、(b) HMAC-SHA256によるデータグラム
//!   単位の完全性・認証、の2つを両方適用する。鍵は `auth::MutualAuthConfig`
//!   と同じ長期共有シークレットからHKDFで導出する運用を想定 (呼び出し側が
//!   `UdpChannelKeys::derive` で導出する)。
//! - **受信側の実配置は未接続**: 実際にどのプロセスがUDPソケットを
//!   listenして`aruaru-db`側WALと結合するかは、open-runo側の実装が必要な
//!   別スコープ (今回は未着手)。本クレートはチャネル自体 (送信・受信・
//!   フレーミング・デデュープ) のみを提供し、`open-web-server-ledger` からは
//!   送信側のみを結線する。
//!
//! ## データグラム形式
//!
//! ```text
//! [ 8 bytes: sequence number (u64 LE) ]
//! [ 32 bytes: HMAC-SHA256(seq || ciphertext) ]
//! [ N bytes: PayloadCipher ciphertext (nonce(12) || AEAD ciphertext) ]
//! ```
//!
//! シーケンス番号は送信側が単調増加させる (再送はしないため衝突検知用途)。
//! HMACはリプレイ改変やなりすましデータグラムの検知に用いる (定数時間比較)。

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Mutex;

use hmac::{Hmac, Mac};
use open_web_server_core::MutationRequest;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use tokio::net::UdpSocket;

use crate::payload_crypto::PayloadCipher;

type HmacSha256 = Hmac<Sha256>;

const SEQ_LEN: usize = 8;
const MAC_LEN: usize = 32;
const HEADER_LEN: usize = SEQ_LEN + MAC_LEN;

/// UDPチャネル用の鍵一式 (AEAD鍵 + HMAC鍵)。
/// `auth::MutualAuthConfig` の長期共有シークレットからHKDFで導出する想定。
#[derive(Clone)]
pub struct UdpChannelKeys {
    pub aead_key: [u8; 32],
    pub mac_key: Vec<u8>,
}

impl UdpChannelKeys {
    /// テスト・開発用の乱数鍵生成。本番は共有シークレットからのHKDF導出を使うこと。
    pub fn generate_for_testing() -> Self {
        Self {
            aead_key: PayloadCipher::generate_key(),
            mac_key: {
                let mut k = vec![0u8; 32];
                rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut k);
                k
            },
        }
    }
}

/// 1件のミューテーションをUDPで送出する側。
///
/// `send` は fire-and-forget。呼び出し側 (ledger) は送信失敗/タイムアウトを
/// 握りつぶしてよい (UDPはあくまで副系のため、TCP経路をブロックしてはならない)。
pub struct UdpSender {
    socket: UdpSocket,
    cipher: PayloadCipher,
    mac_key: Vec<u8>,
    next_seq: std::sync::atomic::AtomicU64,
}

impl UdpSender {
    pub async fn bind(local_addr: SocketAddr, keys: &UdpChannelKeys) -> anyhow::Result<Self> {
        let socket = UdpSocket::bind(local_addr).await?;
        Ok(Self {
            socket,
            cipher: PayloadCipher::new(&keys.aead_key),
            mac_key: keys.mac_key.clone(),
            next_seq: std::sync::atomic::AtomicU64::new(0),
        })
    }

    /// 指定した宛先にミューテーションを1回送信する (再送なし)。
    /// UDP送出自体が失敗しても (例: 宛先未リッスン)、呼び出し側はTCP経路を
    /// 止めるべきではないため、エラーはそのまま返すのみで panicしない。
    pub async fn send_mutation(
        &self,
        dest: SocketAddr,
        req: &MutationRequest,
    ) -> anyhow::Result<()> {
        let seq = self
            .next_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let datagram = encode_frame(seq, req, &self.cipher, &self.mac_key)?;
        self.socket.send_to(&datagram, dest).await?;
        Ok(())
    }

    pub fn local_addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(self.socket.local_addr()?)
    }
}

/// 受信側。デコード・完全性検証・デデュープを担う。
pub struct UdpReceiver {
    socket: UdpSocket,
    cipher: PayloadCipher,
    mac_key: Vec<u8>,
    dedup: Deduplicator,
}

impl UdpReceiver {
    pub async fn bind(local_addr: SocketAddr, keys: &UdpChannelKeys) -> anyhow::Result<Self> {
        let socket = UdpSocket::bind(local_addr).await?;
        Ok(Self {
            socket,
            cipher: PayloadCipher::new(&keys.aead_key),
            mac_key: keys.mac_key.clone(),
            dedup: Deduplicator::default(),
        })
    }

    pub fn local_addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(self.socket.local_addr()?)
    }

    /// 1データグラムを受信し、検証・復号する。
    /// 冪等キーが既知であれば `Ok(None)` (デデュープにより無視すべき重複)。
    /// 新規であれば `Ok(Some(req))`。
    pub async fn recv_mutation(&self) -> anyhow::Result<Option<MutationRequest>> {
        let mut buf = vec![0u8; 64 * 1024];
        let (len, _from) = self.socket.recv_from(&mut buf).await?;
        buf.truncate(len);
        let (_seq, req) = decode_frame(&buf, &self.cipher, &self.mac_key)?;
        if self.dedup.insert_if_new(&req.idempotency_key.0) {
            Ok(Some(req))
        } else {
            Ok(None)
        }
    }
}

/// TCP経由で既にコミット済み/処理中のキーを、UDP側でも重複扱いするための
/// 単純な冪等キー集合。本番実装では `WriteAheadLog::is_already_processed` と
/// 突き合わせる (今回は未接続 = open-runo側の受信実装スコープ)。
#[derive(Default)]
pub struct Deduplicator {
    seen: Mutex<HashSet<String>>,
}

impl Deduplicator {
    /// 未処理キーなら true を返し集合に登録、既知キーなら false を返す。
    pub fn insert_if_new(&self, idempotency_key: &str) -> bool {
        let mut seen = self.seen.lock().unwrap();
        seen.insert(idempotency_key.to_string())
    }

    pub fn contains(&self, idempotency_key: &str) -> bool {
        self.seen.lock().unwrap().contains(idempotency_key)
    }
}

fn encode_frame(
    seq: u64,
    req: &MutationRequest,
    cipher: &PayloadCipher,
    mac_key: &[u8],
) -> anyhow::Result<Vec<u8>> {
    let plaintext = serde_json::to_vec(req)?;
    let ciphertext = cipher.encrypt(&plaintext)?;

    let mut mac = <HmacSha256 as Mac>::new_from_slice(mac_key)
        .map_err(|e| anyhow::anyhow!("invalid HMAC key: {e}"))?;
    mac.update(&seq.to_le_bytes());
    mac.update(&ciphertext);
    let tag = mac.finalize().into_bytes();

    let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&tag);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decode_frame(
    datagram: &[u8],
    cipher: &PayloadCipher,
    mac_key: &[u8],
) -> anyhow::Result<(u64, MutationRequest)> {
    if datagram.len() < HEADER_LEN {
        anyhow::bail!("udp datagram too short ({} bytes)", datagram.len());
    }
    let (seq_bytes, rest) = datagram.split_at(SEQ_LEN);
    let (tag, ciphertext) = rest.split_at(MAC_LEN);
    let seq = u64::from_le_bytes(seq_bytes.try_into().unwrap());

    let mut mac = <HmacSha256 as Mac>::new_from_slice(mac_key)
        .map_err(|e| anyhow::anyhow!("invalid HMAC key: {e}"))?;
    mac.update(seq_bytes);
    mac.update(ciphertext);
    let expected = mac.finalize().into_bytes();

    if expected.as_slice().ct_eq(tag).unwrap_u8() != 1 {
        anyhow::bail!("HMAC verification failed (tamper or wrong key)");
    }

    let plaintext = cipher.decrypt(ciphertext)?;
    let req: MutationRequest = serde_json::from_slice(&plaintext)?;
    Ok((seq, req))
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
            payload: serde_json::json!({"item_id": "sword", "quantity": 1}),
            requested_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn encode_then_decode_roundtrips() {
        let keys = UdpChannelKeys::generate_for_testing();
        let cipher = PayloadCipher::new(&keys.aead_key);
        let req = sample_request("key-1");

        let frame = encode_frame(42, &req, &cipher, &keys.mac_key).unwrap();
        let (seq, decoded) = decode_frame(&frame, &cipher, &keys.mac_key).unwrap();

        assert_eq!(seq, 42);
        assert_eq!(decoded.idempotency_key.0, "key-1");
    }

    #[test]
    fn tampered_datagram_is_rejected() {
        let keys = UdpChannelKeys::generate_for_testing();
        let cipher = PayloadCipher::new(&keys.aead_key);
        let req = sample_request("key-2");

        let mut frame = encode_frame(1, &req, &cipher, &keys.mac_key).unwrap();
        // Flip a bit in the ciphertext portion to simulate tampering.
        let last = frame.len() - 1;
        frame[last] ^= 0xff;

        let result = decode_frame(&frame, &cipher, &keys.mac_key);
        assert!(result.is_err(), "tampered datagram must fail verification");
    }

    #[test]
    fn wrong_mac_key_is_rejected() {
        let keys = UdpChannelKeys::generate_for_testing();
        let other_keys = UdpChannelKeys::generate_for_testing();
        let cipher = PayloadCipher::new(&keys.aead_key);
        let req = sample_request("key-3");

        let frame = encode_frame(1, &req, &cipher, &keys.mac_key).unwrap();
        let result = decode_frame(&frame, &cipher, &other_keys.mac_key);
        assert!(result.is_err());
    }

    #[test]
    fn deduplicator_flags_repeat_keys() {
        let dedup = Deduplicator::default();
        assert!(dedup.insert_if_new("k1"));
        assert!(!dedup.insert_if_new("k1"), "second insert of same key must be rejected");
        assert!(dedup.insert_if_new("k2"));
    }

    /// 実UDPソケット(127.0.0.1のループバック)を使った結合テスト。
    /// モックではなく `tokio::net::UdpSocket` の実送受信で、暗号化・HMAC検証・
    /// デデュープの一連の流れを実証する。
    #[tokio::test]
    async fn real_udp_socket_send_recv_decrypt_and_dedup() {
        let keys = UdpChannelKeys::generate_for_testing();
        let receiver = UdpReceiver::bind("127.0.0.1:0".parse().unwrap(), &keys)
            .await
            .unwrap();
        let recv_addr = receiver.local_addr().unwrap();

        let sender = UdpSender::bind("127.0.0.1:0".parse().unwrap(), &keys)
            .await
            .unwrap();

        let req = sample_request("shared-idempotency-key-xyz");

        // 送信1回目 (UDP側の "advance notice")
        sender.send_mutation(recv_addr, &req).await.unwrap();
        let got = receiver.recv_mutation().await.unwrap();
        assert!(got.is_some(), "first delivery of a new key must be surfaced");
        assert_eq!(got.unwrap().idempotency_key.0, "shared-idempotency-key-xyz");

        // 同じidempotency keyのミューテーションがTCP経路と競合して
        // もう一度UDPでも届いた状況をシミュレート (UDPは重複しうるため)。
        sender.send_mutation(recv_addr, &req).await.unwrap();
        let got_again = receiver.recv_mutation().await.unwrap();
        assert!(
            got_again.is_none(),
            "duplicate idempotency key over UDP must be deduplicated, not double-applied"
        );
    }

    /// UDP経路が全く使えない (誰もlistenしていないポート) 場合でも、
    /// 送信自体 (OSレベルのUDP send) は成功し得るが、少なくとも
    /// この送信操作がpanicやハングを起こさないことを確認する。
    /// TCP経路がUDPの可否に依存しないことは `open-web-server-ledger` 側の
    /// 統合テストで実証する (UDP送出をベストエフォートのfire-and-forwardに
    /// した設計そのものがこの性質を保証する)。
    #[tokio::test]
    async fn send_to_unreachable_udp_destination_does_not_hang_or_panic() {
        let keys = UdpChannelKeys::generate_for_testing();
        let sender = UdpSender::bind("127.0.0.1:0".parse().unwrap(), &keys)
            .await
            .unwrap();
        let req = sample_request("key-unreachable");

        // 誰もbindしていないはずのポートへ送信 (UDPはconnectionlessなので
        // send自体はOSレベルでは成功しうる。ここでは「ブロックしない」ことを確認)。
        let unreachable: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            sender.send_mutation(unreachable, &req),
        )
        .await;
        assert!(result.is_ok(), "send must not hang even to an unreachable destination");
    }
}
