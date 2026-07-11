//! open-web-server-wire: 3層防御 (Defense in Depth) の通信基盤
//!
//! 3Dオンラインゲームの課金アイテムや、金融/クレジットカード情報を扱う
//! ミッションクリティカルなノンストップサーバー向けに、以下3層を独立して積む。
//! 1層が破られても即座に情報漏洩・データ消失に直結しない設計とする。
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │ 第3層  payload_crypto: ChaCha20-Poly1305 AEAD │  アプリ層ペイロード暗号化
//! │        (TLS終端後も、更に平文が流れない)         │
//! ├─────────────────────────────────────────────┤
//! │ 第2層  auth: 相互認証 (mTLS / トークン検証)      │  なりすまし防止
//! ├─────────────────────────────────────────────┤
//! │ 第1層  tls: TLS 1.3 (rustls)                  │  伝送路暗号化
//! └─────────────────────────────────────────────┘
//! ```
//!
//! aruaru-db の `aruaru-wire` crate と同一方針で実装しており、
//! open-web-server ⇔ open-runo ⇔ aruaru-db 間の通信はすべてこの3層を通す。
//!
//! ## `udp_channel` について (2026-07-11 追加、重要な区別)
//!
//! 上記3層は現状すべて**単一のTCPコネクション上のセキュリティレイヤー**
//! (TLS → 相互認証 → ペイロード暗号化) であり、これは変更していない。
//! 別途追加した [`udp_channel`] モジュールは、これとは**直交する新しい能力**
//! ── **伝送経路そのものの冗長化** (単一経路の障害・パケットロスでデータが
//! 失われないようにする) を提供する。3層防御を置き換えるものではなく、
//! UDP経路上でも `payload_crypto::PayloadCipher` によるAEAD暗号化 + 独自HMAC
//! による完全性検証を適用したうえで、TCP経路と並行して使う副系として設計
//! している。詳細・スコープの限界は `udp_channel` モジュールのdocを参照。

pub mod auth;
pub mod payload_crypto;
pub mod tls;
pub mod udp_channel;

pub use auth::MutualAuthConfig;
pub use payload_crypto::PayloadCipher;
pub use tls::TlsServerConfig;
pub use udp_channel::{Deduplicator, UdpChannelKeys, UdpReceiver, UdpSender};
