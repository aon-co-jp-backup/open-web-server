//! ペイロード変換(圧縮+暗号化)のハードウェアアクセラレータ抽象化
//! (2026-07-23、ユーザー指示により新設)。
//!
//! ## 正直な開示・設計判断の経緯
//!
//! ユーザーから「メモリキャッシュへの圧縮+暗号化変換展開を、CPUのみ
//! ならずNPU/GPU/専用ハードウェアアクセラレータでも対応可能にしてほしい
//! (ハードウェアが実在しなくても拡張点として)」との指示を受けた。
//!
//! 日英Web検索で裏取りした結果:
//! - **GPU圧縮**: NVIDIA nvCOMP(Snappy/ZSTD/LZ4等、Blackwell世代では
//!   専用デコンプレッションエンジンで600GB/s)が実在する
//!   ([NVIDIA nvCOMP](https://developer.nvidia.com/nvcomp))。
//! - **GPU暗号化**: CUDA上でのAES高速化は学術研究レベルで実例がある
//!   ([arXiv:1902.05234](https://arxiv.org/abs/1902.05234))。
//! - **CPUハードウェアアクセラレーション**: AES-NI/CLMUL命令は
//!   `chacha20poly1305`/`aes`系のRustCryptoクレートが対応プラット
//!   フォームで自動的に(透過的に)利用する——これは既に本ワークスペース
//!   が使っている`chacha20poly1305`クレートの標準動作であり、追加実装
//!   不要。
//! - **NPU**: 汎用データ圧縮・AEAD暗号化に対応した実用Rustクレート・
//!   ライブラリは調査時点で見当たらなかった。
//!
//! `open-cuda`の`GpuDevice`トレイト(`opencuda-core::device`)は
//! GEMM/Attention等の行列演算カーネル起動を前提とした設計であり、
//! 汎用バイト列の圧縮・AEAD暗号化とは操作の性質が異なる(カーネル
//! ディスパッチに向く処理ではない)ため、本モジュールは`GpuDevice`を
//! 再利用せず、独立した軽量トレイトを新設する。
//!
//! **今回実際に実装したのはCPUバックエンドのみ**。GPU/NPU/専用ハード
//! ウェアアクセラレータは[`AccelBackend`]の列挙子として拡張点を明示
//! するに留め、選択されても実装が無いためCPUへ安全にフォールバックする
//! (存在しない能力を実装済みと偽らない、このエコシステム共通の方針)。

use crate::payload_crypto::PayloadCipher;
use std::io::{Read, Write};

/// ペイロード変換の実行先。CPU以外は現時点で未実装の拡張点
/// (`PayloadAccelerator::new`が選択時にCPUへフォールバックする)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccelBackend {
    /// 実装済み。`flate2`(deflate)による圧縮+`PayloadCipher`
    /// (ChaCha20-Poly1305)による暗号化。AES-NI/CLMUL相当のCPU
    /// ネイティブ命令活用は依存クレート側の既存機能として透過的に働く。
    Cpu,
    /// 未実装の拡張点(NVIDIA nvCOMP等によるGPU圧縮・GPU暗号化が実在
    /// 技術として存在することは調査済みだが、本クレートには未統合)。
    Gpu,
    /// 未実装の拡張点(汎用圧縮・AEAD暗号化に対応した実用ライブラリが
    /// 調査時点で見当たらなかった)。
    Npu,
    /// 未実装の拡張点(専用暗号化アクセラレータカード等)。
    HardwareAccelerator,
}

/// 選択したバックエンドで圧縮+暗号化を行う。
pub struct PayloadAccelerator {
    backend: AccelBackend,
    cipher: PayloadCipher,
}

impl PayloadAccelerator {
    /// `backend`を要求するが、`Cpu`以外は未実装のため自動的に`Cpu`へ
    /// フォールバックする(`tracing::warn!`で可視化、権威パスは止めない
    /// ——既存のUDP冗長経路等と同じ「補助的な最適化の欠如で本処理を
    /// 止めない」設計方針)。
    pub fn new(backend: AccelBackend, cipher: PayloadCipher) -> Self {
        let effective = match backend {
            AccelBackend::Cpu => AccelBackend::Cpu,
            other => {
                tracing::warn!(
                    requested = ?other,
                    "accelerator backend not yet implemented, falling back to Cpu"
                );
                AccelBackend::Cpu
            }
        };
        Self { backend: effective, cipher }
    }

    /// 実際に使われるバックエンド(フォールバック後の値)。
    pub fn backend(&self) -> AccelBackend {
        self.backend
    }

    /// 平文を圧縮してから暗号化する(メモリキャッシュへ格納する形式)。
    pub fn compress_encrypt(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        let compressed = deflate_compress(plaintext)?;
        self.cipher.encrypt(&compressed)
    }

    /// 暗号化されたキャッシュ値を復号してから解凍する。
    pub fn decrypt_decompress(&self, ciphertext: &[u8]) -> anyhow::Result<Vec<u8>> {
        let compressed = self.cipher.decrypt(ciphertext)?;
        deflate_decompress(&compressed)
    }
}

fn deflate_compress(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

fn deflate_decompress(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut decoder = flate2::read::DeflateDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> PayloadCipher {
        PayloadCipher::new(&PayloadCipher::generate_key())
    }

    #[test]
    fn cpu_backend_round_trips_compressed_encrypted_payload() {
        let accel = PayloadAccelerator::new(AccelBackend::Cpu, cipher());
        assert_eq!(accel.backend(), AccelBackend::Cpu);

        let plaintext = b"the quick brown fox jumps over the lazy dog ".repeat(50);
        let encoded = accel.compress_encrypt(&plaintext).unwrap();
        assert!(encoded.len() < plaintext.len(), "repetitive payload should actually compress smaller than the input");

        let decoded = accel.decrypt_decompress(&encoded).unwrap();
        assert_eq!(decoded, plaintext);
    }

    /// 未実装のバックエンドを要求しても、パニックせずCpuへ安全に
    /// フォールバックし、実際に動作することを実証する。
    #[test]
    fn unimplemented_backends_fall_back_to_cpu_and_still_work() {
        for backend in [AccelBackend::Gpu, AccelBackend::Npu, AccelBackend::HardwareAccelerator] {
            let accel = PayloadAccelerator::new(backend, cipher());
            assert_eq!(accel.backend(), AccelBackend::Cpu, "{backend:?} must fall back to Cpu until implemented");

            let plaintext = b"fallback still works correctly";
            let encoded = accel.compress_encrypt(plaintext).unwrap();
            let decoded = accel.decrypt_decompress(&encoded).unwrap();
            assert_eq!(decoded, plaintext);
        }
    }

    #[test]
    fn tampered_ciphertext_is_rejected_after_fallback() {
        let accel = PayloadAccelerator::new(AccelBackend::Cpu, cipher());
        let mut encoded = accel.compress_encrypt(b"sensitive cached value").unwrap();
        let last = encoded.len() - 1;
        encoded[last] ^= 0xFF;
        assert!(accel.decrypt_decompress(&encoded).is_err(), "tampered cache entry must not decrypt successfully");
    }
}
