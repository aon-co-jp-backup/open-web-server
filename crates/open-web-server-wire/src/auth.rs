//! 第2層: 相互認証
//!
//! open-web-server ⇔ open-runo ⇔ aruaru-db のサービス間通信は、
//! TLS だけでなく「両者が正しい相手であること」を毎回検証する。
//! 事前共有した長期鍵から、通信ごとに使い捨てのセッション鍵を HKDF で導出する。

use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use subtle::ConstantTimeEq;

#[derive(Debug, Clone)]
pub struct MutualAuthConfig {
    /// サービス間の長期共有シークレット (例: aruaru-db 発行のサービストークン)
    pub shared_secret: Vec<u8>,
    pub service_id: String,
}

impl MutualAuthConfig {
    /// チャレンジ生成 (呼び出し側)
    pub fn generate_challenge() -> [u8; 32] {
        let mut challenge = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut challenge);
        challenge
    }

    /// チャレンジに対する応答を計算する (HMAC 相当を HKDF で導出)
    pub fn respond(&self, challenge: &[u8]) -> anyhow::Result<[u8; 32]> {
        let hk = Hkdf::<Sha256>::new(Some(challenge), &self.shared_secret);
        let mut okm = [0u8; 32];
        hk.expand(self.service_id.as_bytes(), &mut okm)
            .map_err(|e| anyhow::anyhow!("HKDF expand failed: {e}"))?;
        Ok(okm)
    }

    /// 相手からの応答を定数時間比較で検証する (タイミング攻撃対策)
    pub fn verify(&self, challenge: &[u8], response: &[u8]) -> anyhow::Result<bool> {
        let expected = self.respond(challenge)?;
        Ok(expected.ct_eq(response).into())
    }
}
