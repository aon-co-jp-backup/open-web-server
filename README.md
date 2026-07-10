# open-web-server

**Rust + Poem 製 Web サーバー — 課金アイテム・金融データを「消失させない」ために設計**

3D オンラインゲームの課金アイテム購入やクレジットカード決済のような、
ミッションクリティカルな 24/7/365 ワークロード向けの Web サーバーです。
`open-runo`(Federation Gateway)・`aruaru-db`(分散 Git-on-SQL DB)と
3 層防御通信で連携し、再送・プロセス再起動・ネットワーク瞬断があっても
二重課金やデータ消失が起きない設計になっています。

📖 詳細: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

## 3 本柱

1. **3 層防御通信**(`open-web-server-wire`) — TLS 1.3 + HKDF 相互認証 + ChaCha20-Poly1305
2. **消失しない書き込み**(`open-web-server-ledger`) — Idempotency-Key 必須の WAL 先行書き込み + 3 ホップコミット
3. **open-runo / aruaru-db との密結合** — `Client → open-web-server → open-runo → aruaru-db`
4. **OpenTelemetry トレーシング**(`open-web-server-gateway`) — 各ハンドラのスパンを OTLP または標準出力へエクスポート(詳細は [README-Japan.md](README-Japan.md#4-opentelemetry-によるトレーシング-open-web-server-gatewaytelemetry))

## クイックスタート

```bash
cargo run -p aruaru-server -- --data ./data --raft-id 1   # 1. aruaru-db
cargo run -p open-runo-gateway                             # 2. open-runo
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 \
  cargo run -p open-web-server-gateway                      # 3. open-web-server
```

## 構成(4 クレート)

`open-web-server-core`(ドメイン型/エラー) ・ `open-web-server-wire`(3 層防御通信) ・
`open-web-server-ledger`(冪等 WAL + 3 ホップコミット) ・ `open-web-server-gateway`(Poem ゲートウェイ)。
詳細は [docs/architecture.md](docs/architecture.md) / [docs/integration.md](docs/integration.md)。

## License

Apache-2.0
