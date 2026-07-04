//! 第3層: アプリケーション層ペイロード暗号化
//!
//! TLS はプロキシ/ロードバランサ等で終端されることがあるため、
//! 課金アイテムや金融データそのものは、TLS終端後の平文経路にも
//! 依存しないよう、ChaCha20-Poly1305 (AEAD) でもう一段暗号化する。
//! aruaru-db の `aruaru-wire::payload_crypto` と同一方式。

use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use rand::RngCore;

pub struct PayloadCipher {
    cipher: ChaCha20Poly1305,
}

impl PayloadCipher {
    pub fn new(key: &[u8; 32]) -> Self {
        let key = Key::from_slice(key);
        Self {
            cipher: ChaCha20Poly1305::new(key),
        }
    }

    pub fn generate_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        key
    }

    /// 平文を暗号化し、`nonce || ciphertext` を返す。
    pub fn encrypt(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("encrypt failed: {e}"))?;

        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// `nonce || ciphertext` から平文を復号する。
    pub fn decrypt(&self, data: &[u8]) -> anyhow::Result<Vec<u8>> {
        if data.len() < 12 {
            anyhow::bail!("payload too short to contain nonce");
        }
        let (nonce_bytes, ciphertext) = data.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("decrypt failed (tamper detected?): {e}"))
    }
}
