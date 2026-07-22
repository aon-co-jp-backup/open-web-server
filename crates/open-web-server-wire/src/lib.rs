//! open-web-server-wire: 4層防御 (Defense in Depth) の通信基盤
//!
//! 3Dオンラインゲームの課金アイテムや、金融/クレジットカード情報を扱う
//! ミッションクリティカルなノンストップサーバー向けに、以下4層を独立して積む。
//! 1層が破られても即座に情報漏洩・データ消失・二重処理に直結しない設計とする。
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │ 第4層  replay_guard: seq+timestamp リプレイ対策 │  再送(二重適用)防止
//! ├─────────────────────────────────────────────┤
//! │ 第3層  payload_crypto: ChaCha20-Poly1305 AEAD │  アプリ層ペイロード暗号化
//! │        (TLS終端後も、更に平文が流れない)         │
//! ├─────────────────────────────────────────────┤
//! │ 第2層  auth: 相互認証 (mTLS / トークン検証)      │  なりすまし防止
//! ├─────────────────────────────────────────────┤
//! │ 第1層  tls: TLS 1.3 (rustls)                  │  伝送路暗号化
//! └─────────────────────────────────────────────┘
//! ```
//!
//! 第4層 ([`replay_guard`]) は、第3層のAEADが検知しない「正規の暗号文の
//! そのままの再送」を防ぐ。送信ごとに単調増加するシーケンス番号と
//! UNIXタイムスタンプをAEADのAssociated Dataとして暗号文に暗号学的に
//! 紐付け、受信側が既知シーケンス番号の再受信と許容時刻窓外のタイムスタンプ
//! を拒否する。[`replay_guard::SecureChannel`] は第3層+第4層を1本にまとめた
//! 送受信ヘルパーであり、単体で使う場合は [`payload_crypto::PayloadCipher`]
//! の代わりにこちらを使うことでリプレイ対策込みの暗号化が得られる
//! (課金・決済確定など非冪等な操作の送信に強く推奨)。
//!
//! aruaru-db の `aruaru-wire` crate と同一方針で実装しており、
//! open-web-server ⇔ open-runo ⇔ aruaru-db 間の通信はすべてこの4層を通す。
//!
//! ## `udp_channel` について (2026-07-11 追加、重要な区別)
//!
//! 上記4層は現状すべて**単一のTCPコネクション上のセキュリティレイヤー**
//! (TLS → 相互認証 → ペイロード暗号化 → リプレイ対策) であり、これは変更
//! していない。別途追加した [`udp_channel`] モジュールは、これとは
//! **直交する新しい能力** ── **伝送経路そのものの冗長化** (単一経路の障害・
//! パケットロスでデータが失われないようにする) を提供する。4層防御を
//! 置き換えるものではなく、UDP経路上でも `payload_crypto::PayloadCipher`
//! によるAEAD暗号化 + 独自HMAC + `seq`によるデデュープ(リプレイ対策と
//! 同種の再送保護)を適用したうえで、TCP経路と並行して使う副系として設計
//! している。詳細・スコープの限界は `udp_channel` モジュールのdocを参照。
//!
//! ## `quic_channel` について (2026-07-12 追加)
//!
//! 拡張要件(3)「通信層の四重化」の**③QUIC**の第一実装。`quinn`クレートを
//! 用いてTLS 1.3組み込みの信頼性のある双方向ストリーム伝送を提供する、
//! ①TCP・②UDPとは異なる第3の独立した伝送特性を持つ経路。Multipath QUIC
//! (複数物理経路への分散)は範囲外——単一経路QUICの実装であり、物理経路の
//! マルチホーミングは④ (MPTCP/SCTP、未着手) の担当とする。詳細・スコープの
//! 限界は `quic_channel` モジュールのdocを参照。
//!
//! ## `mptcp_channel` について (2026-07-13 追加、正直な代替実装)
//!
//! 拡張要件(3)「通信層の四重化」の**④(当初 MPTCP/SCTP)**。この
//! Windows開発環境ではカーネルMPTCP/SCTPソケットの作成自体が不可能
//! (Windowsにネイティブサポートが無い)ことを確認した。その代わり、
//! カーネルMPTCP/SCTPと同じ目的(物理経路マルチホーミングによる伝送路
//! 冗長化)をユーザー空間で実現する `aggligator` クレートを用いた実装を
//! 提供する。**本物のカーネルMPTCP/SCTPではない**——調査結果・判断根拠の
//! 詳細は `mptcp_channel` モジュールのdocを参照。
//!
//! ## `rs_smarttcp`について (2026-07-23、独立リポジトリへ切り出し)
//!
//! IOWN/APN(光電融合ネットワーク)のような超低遅延・ジッター無し回線と、
//! Smart-TCP(AI生成通信プロトコル、fast/slowモデルによる判断構造)の
//! 良いとこ取りハイブリッド。実測RTT・ジッターに基づく決定論的な
//! ヒューリスティック判定(訓練済みMLモデルではない)で、リトライ間隔等を
//! 2段階(Fast/Slow)に切り替える。当初このクレート内の`adaptive_channel`
//! モジュールとして実装したが、`Rust-JSON`等と同じ「独立リポジトリとして
//! 切り出し、必要な場所からpath依存する」パターンに合わせ、
//! [`aon-co-jp/RS-SmartTCP`](https://github.com/aon-co-jp/RS-SmartTCP)へ
//! 切り出した。**arXiv論文のSmart-TCPプロトコルそのものの実装ではない**
//! ——詳細・スコープの限界はそちらのdocを参照。
//!
//! ## `accel` について (2026-07-23 追加)
//!
//! メモリキャッシュへの圧縮+暗号化変換を、CPU(実装済み)・GPU/NPU/
//! 専用ハードウェアアクセラレータ(未実装の拡張点)で切り替え可能にする
//! 抽象化。存在しないハードウェア対応を実装済みと偽らず、要求時は
//! 安全にCPUへフォールバックする。詳細・調査結果は`accel`モジュールの
//! docを参照。

pub mod accel;
pub mod auth;
pub mod mptcp_channel;
pub mod payload_crypto;
pub mod quic_channel;
pub mod replay_guard;
pub mod tls;
pub mod udp_channel;

pub use accel::{AccelBackend, PayloadAccelerator};
pub use auth::MutualAuthConfig;
pub use rs_smarttcp::{AdaptiveMode, AdaptivePolicy, NetworkQualityMonitor};
pub use mptcp_channel::{send_mutation_over_mptcp, MptcpServer};
pub use payload_crypto::PayloadCipher;
pub use quic_channel::{
    insecure_client_config_trusting, send_mutation_over_quic, QuicServer, QuicServerConfig,
};
pub use replay_guard::{ReplayGuard, SecureChannel};
pub use tls::{build_tenant_server_config, TenantCertResolver, TlsServerConfig};
pub use udp_channel::{Deduplicator, UdpChannelKeys, UdpReceiver, UdpSender};
