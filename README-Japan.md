# open-web-server

## 課金アイテムと金融データを「消さない」ための Rust + Poem 製 Webサーバー

open-web-server は、3Dオンラインゲームのアイテム課金や、クレジットカード決済のような
金融データを扱う、24時間365日ノンストップ運用のミッションクリティカルな Webサーバーです。
**Rust + Poem** で実装し、aruaru-db・open-runo と連携する4層防御アーキテクチャにより、
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

### 1. 4層防御通信 (`open-web-server-wire`)

aruaru-db の `aruaru-wire` と同一方針で、通信経路を4層で保護します。

| 層 | 技術 | 目的 |
|---|---|---|
| 第1層 | TLS 1.3 (rustls) | 伝送路暗号化 |
| 第2層 | HKDFベースのチャレンジ&レスポンス | サービス間相互認証（なりすまし防止） |
| 第3層 | ChaCha20-Poly1305 (AEAD) | アプリ層ペイロード暗号化（TLS終端後も平文にしない） |
| 第4層 | seq/timestamp リプレイ対策 (`replay_guard`, 2026-07-11追加) | 正規暗号文の再送(二重課金・二重付与)防止 |

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
        (4層防御通信)      (4層防御通信)
```

- open-web-server: クライアント向け入口（REST/GraphQL、WAL先行書き込み）
- open-runo: 認証・Rate Limit・監査ログを一元管理する Federation Gateway
- aruaru-db: Git-on-SQL の分散データベース。コミットごとに監査可能なハッシュを発行

詳細は [`docs/architecture.md`](docs/architecture.md) と
[`docs/integration.md`](docs/integration.md) を参照してください。

### 4. OpenTelemetry によるトレーシング (`open-web-server-gateway::telemetry`)

`grant_item`/`charge` の各ハンドラは `tracing::instrument` でスパン化されており、
`tracing-opentelemetry` を通じて OpenTelemetry のトレースとしてエクスポートされます。

- `OTEL_EXPORTER_OTLP_ENDPOINT` を設定すると OTLP/HTTP (protobuf) で
  Collector へ送信します(本番/ステージング向け)。
- 未設定の場合は標準出力にスパンを書き出します(ローカル開発・Collector
  未起動時のフォールバック)。

`Client → open-web-server → open-runo → aruaru-db` の一連の呼び出しを
分散トレースとして追跡する土台であり、`open-runo`/`aruaru-db` 側が同様の
Exporter 設定に対応すればエンドツーエンドのトレースにつながります(両リポジトリ
側の対応状況は未確認)。

### 5. UDP-IP 冗長経路 (`open-web-server-wire::udp_channel`, 2026-07-11)

課金・金融トランザクションがネットワーク上で消失しないよう、既存のTCP経由
権威パスと**並行して**同じミューテーションをUDPでも即時送出する冗長経路を
追加しました。4層防御通信(TLS/相互認証/ペイロード暗号化/リプレイ対策)を
置き換えるものではなく、それとは別の「伝送経路そのものの複線化」です。

- `open-web-server-ledger::Ledger::commit()` は WAL 先行書き込み直後に
  `tokio::spawn` でUDP送信をfire-and-forget発火し、TCP経由の権威コミット
  (`forward_with_retry`)は一切ブロックしません。UDP送出が失敗・宛先未到達
  でもコミット自体は成功します。
- データグラムは `PayloadCipher` (ChaCha20-Poly1305 AEAD) で暗号化し、
  HMAC-SHA256でデータグラム単位の完全性・認証を付与します(UDPにはTLSが
  無いため)。
- 受信側は `IdempotencyKey` によるデデュープ (`Deduplicator`) を行うため、
  同じミューテーションがTCP・UDP双方で届いても二重処理にはなりません。

**スコープの限界(正直な記載)**: UDP側の再送機構は未実装で「送りっぱなしの
即時通知」に留まります。目標アーキテクチャの「主系TCP+副系TCP+UDP」の
三重化のうち今回はUDP1系のみの第一実装であり、副系TCPは未着手です。
受信側の実配置(open-runo側でのUDPリスナー実装)も別スコープで、本リポジトリ
は送信側の結線と検証用受信ロジックの提供までです。詳細は
[`docs/architecture.md`](docs/architecture.md#冗長化された伝送経路-tcp-ip--udp-ip-open-web-server-wireudp_channel-2026-07-11)
を参照してください。

### 6. 目標アーキテクチャ: 通信層・DBの四重化

（2026-07-11改訂: 当初「TCP+UDPの三層三重」構想を、より現実的な到達点を
調査したうえで「四層四重」へ拡張。ユーザー指示・実装は段階的に進める
目標アーキテクチャであり、以下は最終形の全体像。詳細・出典は
[`CLAUDE.md`](CLAUDE.md#拡張要件2026-07-11ユーザー指示目標アーキテクチャ実装は段階的に)
を参照。）

3Dオンラインゲームの課金アイテム・金融データ・証券データ・クレジットカード
情報がネットワーク上で紛失しないよう、`open-web-server`・`poem-cosmo-tauri`
(または`open-runo`)・`PostgreSQL`・`aruaru-db`・`open-raid-z` を組み合わせ、
以下の仕組みを目標とする。

- **通信層の四重化**: ①TCP-IP、②UDP-IP、③QUIC(理想的にはMPQUIC)、
  ④Multipath TCP(MPTCP)またはSCTP、という性質の異なる4つの伝送方式を
  並行させる。
- **DATABASE書き込みの四重化**: ①PostgreSQL(ACID＝原子性・一貫性・独立性・
  永続性を保証するトランザクション特性)、②aruaru-db、③マルチ
  リージョン同期レプリケーション、④独立した監査用トランザクションログ、
  という4つの独立した永続化先へ同一トランザクションを反映する。

**現状の実装状況(正直な記載、2026-07-11時点)**: 通信層は①TCP-IPと
②UDP-IPのみ実装済み(本リポジトリの[UDP-IP冗長経路](#5-udp-ip-冗長経路-open-web-server-wireudp_channel-2026-07-11)、
再送機構なしのfire-and-forget)。③QUIC/MPQUICと④MPTCP/SCTPは未着手。
DB書き込みの四重化(PostgreSQL・aruaru-db・マルチリージョン同期
レプリケーション・独立監査ログ)も未着手。VersionLessAPI+Gitバージョン
管理のハイブリッドおよび`open-raid-z`との連携も同様に未着手で、いずれも
今後のパスで段階的に実装していく。

**次回新規開発予定: aruaru-dbコミット × ZFSスナップショットの連携
(2026-07-11、ネット調査+ユーザー判断)**: `aruaru-db`のRaft分散合意
レプリケーションと`open-raid-z`(ZFS類似)のスナップショットを直接統合
する確立された技術は既存文献からは見つからなかったが、**これは新規性
のある発見であり実装可能と判断し、次回パス以降の新規開発項目とする**。
着眼点: aruaru-dbのRaftログエントリ(コミット)確定タイミングに合わせて
ZFS類似スナップショットを取得し、アプリケーション層(Gitコミット履歴)
とファイルシステム層(ZFSスナップショット)という2つの独立した版管理
機構の間にトランザクション単位の対応関係を持たせる——詳細は
[`CLAUDE.md`](CLAUDE.md#拡張要件2026-07-11ユーザー指示目標アーキテクチャ実装は段階的に)
を参照。

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
│   ├── open-web-server-wire/     # 4層防御通信 (TLS / 相互認証 / ペイロード暗号化 / リプレイ対策)
│   ├── open-web-server-ledger/   # 冪等WAL + 3層コミットパイプライン
│   └── open-web-server-gateway/  # Poem製 Webゲートウェイ (実行バイナリ)
├── docs/
│   ├── architecture.md
│   └── integration.md
└── Cargo.toml (workspace)
```

## ロードマップ

- [ ] `open-cosmo` 共通クレートへの `MutationRequest`/`MutationReceipt` 切り出し
  (着手前に `open-runo`/`aruaru-db` 側の対応状況を確認する方針)
- [ ] GraphQL エンドポイント (`async-graphql` 等) の追加
- [ ] Rust → WASM 製の管理画面（open-runo/aruaru-db の管理UIと統一デザイン。
  2026-07-10のスタック転換により Tauri ではなく Rust/WASM で実装する）
- [x] OpenTelemetry連携によるトレーシング(`open-web-server-gateway` 側は実装済み。
  `open-runo`/`aruaru-db` 側の対応込みのE2Eトレースは今後の課題)
- [x] UDP-IP冗長経路の第一実装(`open-web-server-wire::udp_channel`。再送機構・
  副系TCP・open-runo側受信リスナーは未着手で今後の課題)

## ライセンス

Apache-2.0
