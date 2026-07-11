# open-web-server

**Rust + Poem 製 Web サーバー — 課金アイテム・金融データを「消失させない」ために設計**

3D オンラインゲームの課金アイテム購入やクレジットカード決済のような、
ミッションクリティカルな 24/7/365 ワークロード向けの Web サーバーです。
`open-runo`(Federation Gateway)・`aruaru-db`(分散 Git-on-SQL DB)と
4 層防御通信で連携し、再送・プロセス再起動・ネットワーク瞬断があっても
二重課金やデータ消失が起きない設計になっています。

📖 詳細: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

## 6 本柱

1. **4 層防御通信**(`open-web-server-wire`) — TLS 1.3 + HKDF 相互認証 + ChaCha20-Poly1305 + seq/timestamp リプレイ対策
2. **消失しない書き込み**(`open-web-server-ledger`) — Idempotency-Key 必須の WAL 先行書き込み + 3 ホップコミット
3. **open-runo / aruaru-db との密結合** — `Client → open-web-server → open-runo → aruaru-db`
4. **OpenTelemetry トレーシング**(`open-web-server-gateway`) — 各ハンドラのスパンを OTLP または標準出力へエクスポート(詳細は [README-Japan.md](README-Japan.md#4-opentelemetry-によるトレーシング-open-web-server-gatewaytelemetry))
5. **UDP-IP 冗長経路**(`open-web-server-wire::udp_channel`, 2026-07-11) — TCP経由の権威コミットと並行して、暗号化+HMAC付きのUDP即時通知をベストエフォートで送出(再送なし・第一実装。詳細は [README-Japan.md](README-Japan.md#5-udp-ip-冗長経路-open-web-server-wireudp_channel-2026-07-11))
6. **目標アーキテクチャ: 通信層・DB書き込みの四重化**(2026-07-11改訂) — 課金/金融/証券/クレジットカードデータをネットワーク上で失わないため、通信層は TCP-IP・UDP-IP・QUIC/MPQUIC・MPTCP/SCTP の4方式、DB書き込みは PostgreSQL(ACID＝原子性・一貫性・独立性・永続性を保証するトランザクション特性)・aruaru-db・マルチリージョン同期レプリケーション・独立監査ログの4系統を目標とする。現状は①②(TCP-IP・UDP-IP、再送なしfire-and-forget)のみ実装済みで、③④および四重DB書き込みは未着手(詳細は [README-Japan.md](README-Japan.md#6-目標アーキテクチャ-通信層dbの四重化) と [CLAUDE.md](CLAUDE.md#拡張要件2026-07-11ユーザー指示目標アーキテクチャ実装は段階的に))。**次回新規開発予定**: aruaru-dbコミット×ZFSスナップショット(open-raid-z)の連携——確立技術は無いが新規性ある実装可能なアイデアとして次回パスで着手予定(詳細は同上)。

## クイックスタート

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1   # 1. aruaru-db
cargo run -p open-runo-gateway                             # 2. open-runo
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 \
  cargo run -p open-web-server-gateway                      # 3. open-web-server
```

## 構成(4 クレート)

`open-web-server-core`(ドメイン型/エラー) ・ `open-web-server-wire`(4 層防御通信) ・
`open-web-server-ledger`(冪等 WAL + 3 ホップコミット) ・ `open-web-server-gateway`(Poem ゲートウェイ)。
詳細は [docs/architecture.md](docs/architecture.md) / [docs/integration.md](docs/integration.md)。

## License

Apache-2.0
