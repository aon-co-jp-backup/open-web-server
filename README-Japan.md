# open-web-server

## 課金アイテムと金融データを「消さない」ための Rust + Poem 製 Webサーバー

open-web-server は、3Dオンラインゲームのアイテム課金や、クレジットカード決済のような
金融データを扱う、24時間365日ノンストップ運用のミッションクリティカルな Webサーバーです。
**Rust + Poem** で実装し、aruaru-db・open-runo と連携する3層防御アーキテクチャにより、
ネットワーク瞬断・プロセス再起動・リトライが起きても「二重課金」も「データ消失」も
起こさない設計を目指します。

📖 他の言語: [日本語](README-Japan.md) / [English](README-English.md) /
[中文](README-Chinese.md) / [한국어](README-Korea.md) / [Español](README-Spain.md) /
[Français](README-France.md) / [Deutsch](README-Germany.md) / [Italiano](README-Italy.md) /
[Русский](README-Russia.md) / [العربية](README-Arabic.md)

---

## なぜ open-web-server が必要か

一般的な Webサーバーの課金/決済処理には、次のようなリスクが残ります。

| リスク | 内容 |
|---|---|
| 二重課金 | クライアントの再送・タイムアウトリトライで同じ決済が2回実行される |
| データ消失 | サーバープロセスがクラッシュした瞬間の書き込みが失われる |
| 伝送路の弱点 | TLS終端後（ロードバランサ配下など）は平文でデータが流れる |
| なりすまし | サービス間通信で「本当に正しい相手か」を都度検証していない |
| 障害の握りつぶし | DB書き込みが失敗しているのに、クライアントには成功と返してしまう |

open-web-server は、これらすべてに対して明示的な対策を組み込みます。

## 3つの柱

### 1. 3層防御通信 (`open-web-server-wire`)

aruaru-db の `aruaru-wire` と同一方針で、通信経路を3層で保護します。

| 層 | 技術 | 目的 |
|---|---|---|
| 第1層 | TLS 1.3 (rustls) | 伝送路暗号化 |
| 第2層 | HKDFベースのチャレンジ&レスポンス | サービス間相互認証（なりすまし防止） |
| 第3層 | ChaCha20-Poly1305 (AEAD) | アプリ層ペイロード暗号化（TLS終端後も平文にしない） |

### 2. 消失しない書き込み (`open-web-server-ledger`)

すべての課金・決済リクエストは `Idempotency-Key` を必須とし、以下の順で確定します。

1. クライアントが冪等キー付きでリクエスト
2. open-web-server がローカル WAL に先行書き込み（プロセス再起動時にリプレイ可能）
3. open-runo（Graph Federation Gateway）経由で aruaru-db へ転送
4. aruaru-db が Raft 分散合意でコミットし、`commit_id` を発行
5. `commit_id` を受け取るまで「確定」をクライアントに返さない

途中で通信が失敗しても指数バックオフで自動リトライし、同じ冪等キーの再送は
常に同じ結果を返すため、二重課金・二重付与は起きません。

### 3. aruaru-db / open-runo との緊密な連携

```text
Client → open-web-server → open-runo → aruaru-db
        (3層防御通信)      (3層防御通信)
```

- open-web-server: クライアント向け入口（REST/GraphQL、WAL先行書き込み）
- open-runo: 認証・Rate Limit・監査ログを一元管理する Federation Gateway
- aruaru-db: Git-on-SQL の分散データベース。コミットごとに監査可能なハッシュを発行

詳細は [`docs/architecture.md`](docs/architecture.md) と
[`docs/integration.md`](docs/integration.md) を参照してください。

---

## クイックスタート

```bash
# 1. aruaru-db を起動
cargo run -p aruaru-server -- --data ./data --raft-id 1

# 2. open-runo を起動
cargo run -p open-runo-gateway

# 3. open-web-server を起動
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

```bash
# アイテム付与 (冪等キー必須)
curl -X POST http://localhost:8080/api/v1/items/grant \
  -H "Idempotency-Key: 11111111-1111-1111-1111-111111111111" \
  -H "Content-Type: application/json" \
  -d '{
    "idempotency_key": "11111111-1111-1111-1111-111111111111",
    "account_id": "user-42",
    "item_id": "sword_of_dawn",
    "quantity": 1
  }'
```

## プロジェクト構成

```text
open-web-server/
├── crates/
│   ├── open-web-server-core/     # ドメインモデル・エラー型
│   ├── open-web-server-wire/     # 3層防御通信 (TLS / 相互認証 / ペイロード暗号化)
│   ├── open-web-server-ledger/   # 冪等WAL + 3層コミットパイプライン
│   └── open-web-server-gateway/  # Poem製 Webゲートウェイ (実行バイナリ)
├── docs/
│   ├── architecture.md
│   └── integration.md
└── Cargo.toml (workspace)
```

## ロードマップ

- [ ] `open-cosmo` 共通クレートへの `MutationRequest`/`MutationReceipt` 切り出し
- [ ] GraphQL エンドポイント (`poem-openapi` / `async-graphql`) の追加
- [ ] Tauri製の管理画面（open-runo/aruaru-db の管理UIと統一デザイン）
- [ ] OpenTelemetry連携によるE2Eトレーシング

## ライセンス

Apache-2.0
