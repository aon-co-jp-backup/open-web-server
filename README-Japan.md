# open-web-server

## 課金アイテムと金融データを「消さない」ための Rust + tokio/hyper 製 Webサーバー

open-web-server は、3Dオンラインゲームのアイテム課金や、クレジットカード決済のような
金融データを扱う、24時間365日ノンストップ運用のミッションクリティカルな Webサーバーです。
**Rust + tokio/hyper**(ルーティング/ハンドラのAPI形状は旧Poem実装と互換だが、
2026-07-10にPoemパッケージへの依存自体は解消済み)で実装し、aruaru-db・open-runo と
連携する4層防御アーキテクチャにより、ネットワーク瞬断・プロセス再起動・リトライが
起きても「二重課金」も「データ消失」も起こさない設計を目指します。

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

**現状の実装状況(正直な記載、2026-07-12更新)**: 通信層は①TCP-IPと
②UDP-IPに加え、**③QUICを実装**した
(`open-web-server-wire::quic_channel`、`quinn`クレート、TLS1.3組み込み・
自己署名証明書は開発/検証用。実UDPソケット上での実TLSハンドシェイク+
双方向ストリーム往復を結合テストで検証済み。Multipath QUIC=MPQUICへの
拡張は範囲外、単一経路QUICの第一実装)。**④MPTCP/SCTPは調査の上、
Windows開発環境ではカーネル実装・実ソケット検証が不可能と判明**
(正直なブロッカー)。同じ目的(物理経路マルチホーミング)をユーザー空間で
実現する`aggligator`クレートによる代替(`open-web-server-wire::mptcp_channel`、
本物のカーネルMPTCP/SCTPではない旨を明記)を実装し、実ループバックTCP
ソケットでのラウンドトリップ結合テストで検証済み(2026-07-13)。
DB書き込みの四重化のうち**①PostgreSQLのWAL実装を追加**した
(`open-web-server-ledger::PostgresWal`、`sqlx`クレート、実`BEGIN`/
`COMMIT`トランザクション境界+`ON CONFLICT DO NOTHING`による冪等な
先行書き込み。**ただしこのサンドボックス環境には到達可能な
PostgreSQLインスタンスが無く、実DB接続での検証はできていない**——
SQL構築ロジックの単体テストと、`DATABASE_URL`が設定された環境でのみ
動く`#[ignore]`付き統合テストの2段構えで検証可能性のみ確保)。
**④独立監査トランザクションログを実装**した
(`open-web-server-ledger::audit_log::FileAuditLog`、2026-07-13)。
PostgreSQL/aruaru-db/マルチリージョン同期レプリケーションのいずれとも
技術的に独立した追記専用ファイルへ、コミット試行ごとに1レコード
(SHA-256チェックサム付き)を追記する。`scan_and_verify()`でサイレント
破損を検出し、`reconcile()`でWAL側の確定済みキー集合と突き合わせて
「監査ログにあるがWAL未確定」「監査ログ内で同一キーが重複」を検出
できる。`Ledger::enable_audit_log(path)`で任意有効化し、`commit()`内で
WAL先行書き込み直後に追記(失敗しても権威パスはブロックしない設計)。
実ファイルI/Oでの往復・チェックサム破損検出・突き合わせレポート・
`Ledger::commit`経由の統合の計4テストで実証済み。
**③マルチリージョン同期レプリケーションも実装**した
(`open-web-server-ledger::multi_region::MultiRegionReplicator`、
2026-07-13)。実SQLiteファイル2つ以上を「リージョン」の代替として使い、
コミット時に**同期的に全リージョンへ書き込み、全員のACKを待ってから
成功を返す**(UDP冗長経路のfire-and-forgetとは対照的)。障害ポリシーは
デフォルトの厳格モード(1系統でも失敗すれば全体失敗)と`with_quorum(n)`
によるN-of-M縮退モードの両方に対応。実ファイルI/Oでの正常系・縮退系・
全滅系の計4テストで検証済み。**これでDB書き込みの四重化は①②③④の
4系統すべて実装済み**(①のみ実PostgreSQL接続での検証は未実施)。
**②aruaru-db×ZFSスナップショット連携も aruaru-db 側で実装完了**
(`aruaru-dist::snapshot_pairing`、Raftコミット確定タイミングに合わせて
`open-raid-z`のスナップショットを実際に取得、実RAID-Z2プールで検証済み
——詳細は aruaru-db 側の`CLAUDE.md`参照)。
VersionLessAPI+Gitバージョン管理のハイブリッドは、書き込み側
(commit_idをレスポンスに含める)は既に機能しているが、読み出し側
(commit_id指定で過去状態を問い合わせるクエリAPI)が未着手——
`open-runo`側との連携が必要なため次回パスで着手予定。
`open-raid-z`との連携(ディスク冗長化基盤としての直接組み込み)も
同様に未着手。

### 7. 静的ファイル + PHP配信(Apache+Nginxハイブリッド配信エンジンへの第一歩、2026-07-20)

`open-web-server-gateway`に、既存のAPIバックエンド用途(`tenant_router`)
とは独立した「静的ファイル/PHPサイト向けvhost」機構(`static_files`/
`php_server`/`web_vhost`)を追加。ホスト名ごとにdocrootを割り当て、拡張子
から静的アセットと判定できるパスは直接ファイル配信(ディレクトリ
トラバーサル対策込み)、それ以外は`php -S`(PHPビルトインdevサーバ)への
サブプロセス起動+リバースプロキシで処理する。実際に`audiocafe.tokyo`
(PHP製の既存サイト)を本サーバー経由で配信できることを実HTTP経由で
検証済み(詳細は本ファイルのHANDOFF節参照)。設定は`web_vhosts.toml`
(TOML宣言、`domains.toml`と同じ作法)または管理API
(`POST /admin/web-vhosts`)から動的に追加できる。

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
│   └── open-web-server-gateway/  # tokio/hyper製 Webゲートウェイ (実行バイナリ、Poem非依存/Poem互換API)
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
