//! 第4層: リプレイ(再送)攻撃対策
//!
//! 第3層のAEAD (`payload_crypto`) は改ざん検知と機密性を提供するが、
//! 正規に暗号化された暗号文をネットワーク上で捕捉し、そのまま再送する
//! 「リプレイ攻撃」は防がない。課金アイテムの付与や決済確定のような
//! 冪等でない操作が、暗号文の再送だけで二重に適用されてしまう恐れがある。
//!
//! 第4層は、送信ごとに単調増加するシーケンス番号とUNIXタイムスタンプを
//! AEADのAssociated Data (AAD) として暗号文に暗号学的に紐付けたうえで、
//! 受信側が (1) 既知シーケンス番号の再受信を拒否し、(2) 許容時刻窓外の
//! タイムスタンプを拒否する。AADに含めているため、攻撃者がseq/timestamp
//! だけを差し替えて再送することもできない(AEADタグ検証で失敗する)。

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;

/// タイムスタンプの許容ずれ幅 (秒)。これを超えて古い/未来のパケットは拒否する。
pub const FRESHNESS_WINDOW_SECS: u64 = 30;

/// 追跡するシーケンス番号の最大保持件数 (メモリ上限)。超過分は最も古いものから破棄する。
const MAX_TRACKED_SEQ: usize = 10_000;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// シーケンス番号の既知集合とタイムスタンプの鮮度を検証する再送検知器。
pub struct ReplayGuard {
    seen: BTreeSet<u64>,
}

impl ReplayGuard {
    pub fn new() -> Self {
        Self {
            seen: BTreeSet::new(),
        }
    }

    /// シーケンス番号とタイムスタンプを検証する。
    /// 既知のシーケンス番号 (再送) か、許容時刻窓外なら拒否する。
    pub fn check_and_record(&mut self, seq: u64, timestamp_secs: u64) -> anyhow::Result<()> {
        let diff = now_secs().abs_diff(timestamp_secs);
        if diff > FRESHNESS_WINDOW_SECS {
            anyhow::bail!("timestamp outside freshness window (diff={diff}s)");
        }
        if !self.seen.insert(seq) {
            anyhow::bail!("replayed sequence number: {seq}");
        }
        if self.seen.len() > MAX_TRACKED_SEQ {
            if let Some(&oldest) = self.seen.iter().next() {
                self.seen.remove(&oldest);
            }
        }
        Ok(())
    }
}

impl Default for ReplayGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// 第3層(AEAD)+第4層(リプレイ対策)を1本にまとめた送受信ヘルパー。
///
/// ワイヤーフォーマット: `seq:u64(BE) || timestamp:u64(BE) || nonce:12B || ciphertext`
pub struct SecureChannel {
    cipher: ChaCha20Poly1305,
    next_seq: u64,
    guard: ReplayGuard,
}

impl SecureChannel {
    pub fn new(key: &[u8; 32]) -> Self {
        let key = Key::from_slice(key);
        Self {
            cipher: ChaCha20Poly1305::new(key),
            next_seq: 0,
            guard: ReplayGuard::new(),
        }
    }

    /// 平文を暗号化し、`seq || timestamp || nonce || ciphertext` を返す。
    /// 送信のたびにシーケンス番号を進める。
    pub fn encrypt(&mut self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        let seq = self.next_seq;
        self.next_seq += 1;
        let timestamp = now_secs();

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let mut aad = Vec::with_capacity(16);
        aad.extend_from_slice(&seq.to_be_bytes());
        aad.extend_from_slice(&timestamp.to_be_bytes());

        let ciphertext = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|e| anyhow::anyhow!("encrypt failed: {e}"))?;

        let mut out = Vec::with_capacity(8 + 8 + 12 + ciphertext.len());
        out.extend_from_slice(&seq.to_be_bytes());
        out.extend_from_slice(&timestamp.to_be_bytes());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// `seq || timestamp || nonce || ciphertext` を検証・復号する。
    /// AEADタグ検証(改ざん検知)を通過した後にリプレイ検証を行う。
    pub fn decrypt(&mut self, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        if data.len() < 8 + 8 + 12 {
            anyhow::bail!("payload too short for seq+timestamp+nonce header");
        }
        let (seq_bytes, rest) = data.split_at(8);
        let (ts_bytes, rest) = rest.split_at(8);
        let (nonce_bytes, ciphertext) = rest.split_at(12);

        let seq = u64::from_be_bytes(seq_bytes.try_into().unwrap());
        let timestamp = u64::from_be_bytes(ts_bytes.try_into().unwrap());
        let nonce = Nonce::from_slice(nonce_bytes);

        let mut aad = Vec::with_capacity(16);
        aad.extend_from_slice(seq_bytes);
        aad.extend_from_slice(ts_bytes);

        let plaintext = self
            .cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|e| anyhow::anyhow!("decrypt failed (tamper detected?): {e}"))?;

        self.guard.check_and_record(seq, timestamp)?;

        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_succeeds() {
        let key = [7u8; 32];
        let mut tx = SecureChannel::new(&key);
        let mut rx = SecureChannel::new(&key);

        let frame = tx.encrypt(b"charge:100yen").unwrap();
        let plaintext = rx.decrypt(&frame).unwrap();
        assert_eq!(plaintext, b"charge:100yen");
    }

    #[test]
    fn replayed_frame_is_rejected() {
        let key = [7u8; 32];
        let mut tx = SecureChannel::new(&key);
        let mut rx = SecureChannel::new(&key);

        let frame = tx.encrypt(b"charge:100yen").unwrap();
        rx.decrypt(&frame).unwrap();

        // 同一フレームをそのまま再送 (捕捉されたパケットの単純リプレイ)
        let err = rx.decrypt(&frame).unwrap_err();
        assert!(err.to_string().contains("replayed"));
    }

    #[test]
    fn stale_timestamp_is_rejected() {
        let key = [9u8; 32];
        let mut rx = SecureChannel::new(&key);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));

        let seq: u64 = 0;
        let stale_timestamp: u64 = 0; // 1970年、明らかに許容窓外
        let nonce_bytes = [1u8; 12];
        let nonce = Nonce::from_slice(&nonce_bytes);

        let mut aad = Vec::new();
        aad.extend_from_slice(&seq.to_be_bytes());
        aad.extend_from_slice(&stale_timestamp.to_be_bytes());

        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: &b"grant_item"[..],
                    aad: &aad,
                },
            )
            .unwrap();

        let mut frame = Vec::new();
        frame.extend_from_slice(&seq.to_be_bytes());
        frame.extend_from_slice(&stale_timestamp.to_be_bytes());
        frame.extend_from_slice(&nonce_bytes);
        frame.extend_from_slice(&ciphertext);

        let err = rx.decrypt(&frame).unwrap_err();
        assert!(err.to_string().contains("freshness window"));
    }

    #[test]
    fn tampered_sequence_breaks_aad_binding() {
        let key = [3u8; 32];
        let mut tx = SecureChannel::new(&key);
        let mut rx = SecureChannel::new(&key);

        let mut frame = tx.encrypt(b"charge:100yen").unwrap();
        // seq(先頭8B)を書き換えて再送 -> AADが変わりAEADタグ検証で失敗するはず
        frame[7] ^= 0xFF;

        let err = rx.decrypt(&frame).unwrap_err();
        assert!(err.to_string().contains("tamper detected"));
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let mut tx = SecureChannel::new(&[1u8; 32]);
        let mut rx = SecureChannel::new(&[2u8; 32]);

        let frame = tx.encrypt(b"charge:100yen").unwrap();
        assert!(rx.decrypt(&frame).is_err());
    }
}
