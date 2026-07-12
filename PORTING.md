# PORTING.md — open-web-server お引越しファイル

> このファイル1枚で、他プロジェクトへ `open-web-server` を導入・移設できます。
> 対象バージョン: workspace 0.1.0(4クレート / 26テスト(1件は要ライブPostgreSQLの
> `#[ignore]`)、2026-07-13実測。open-web-server-wireにQUIC(`quinn`)伝送路、
> MPTCP/SCTPのユーザー空間代替(`aggligator`)伝送路、
> open-web-server-ledgerにPostgreSQL WAL(`sqlx`)を追加)。
> 最新のクレート数・テスト数は `CLAUDE.md` の「現状」節を参照。
> 最終更新: 2026-07-13

---

## 1. open-web-server とは(30秒版)

3Dオンラインゲームの課金アイテムや金融/クレジットカードデータを扱う
ミッションクリティカルなWebサーバー。**Rust + Poem**(`open-web-server-gateway`
はまだPoem依存、tokio/hyper直接実装への移行は未着手 — `CLAUDE.md`参照)で、
4層防御通信・冪等WAL + 3ホップコミット・UDP-IP冗長経路により、
ネットワーク瞬断やリトライがあっても二重課金・データ消失が起きないことを
目指す。単体では動かず、`open-runo`(Federation Gateway)・`aruaru-db`
(分散Git-on-SQL DB)との連携が前提。

## 2. 持っていくもの(ファイル一覧)

```
open-web-server/
├── Cargo.toml / Cargo.lock      ← workspace定義(バージョン固定)
├── crates/
│   ├── open-web-server-core/     ← ドメインモデル・エラー型
│   ├── open-web-server-wire/     ← 4層防御通信 + UDP-IP冗長経路(udp_channel) + QUIC冗長経路(quic_channel) + MPTCP/SCTP代替冗長経路(mptcp_channel、aggligatorベース)
│   ├── open-web-server-ledger/   ← 冪等WAL + 3ホップコミット + PostgreSQL WAL実装(postgres_wal)
│   └── open-web-server-gateway/  ← Poem製ゲートウェイ(実行バイナリ)
├── docs/                        ← architecture.md / integration.md
└── PORTING.md                   ← 本ファイル
```

丸ごと移設する場合はフォルダごとコピーして `cargo test --workspace`
(18テストが通れば移設成功)。以下はライブラリとして個別に使う場合。

## 3. 依存の書き方(新プロジェクトのCargo.toml)

```toml
[dependencies]
open-web-server-core   = { path = "../open-web-server/crates/open-web-server-core" }
open-web-server-wire   = { path = "../open-web-server/crates/open-web-server-wire" }
open-web-server-ledger = { path = "../open-web-server/crates/open-web-server-ledger" }

tokio = { version = "1", features = ["full"] }
```

`open-web-server-gateway`(バイナリ)自体を移設したい場合は
`crates/open-web-server-gateway`ごとコピーし、`OPEN_RUNO_ENDPOINT`を
環境変数で指定して起動する。

## 4. 組み込みレシピ

### 4.1 冪等WAL + 3ホップコミット(TCP経由、権威パス)

```rust
use open_web_server_ledger::{Ledger, LedgerConfig};
use std::{sync::Arc, time::Duration};

let ledger = Ledger::new(
    LedgerConfig {
        open_runo_endpoint: "https://runo.internal:8443".to_string(),
        max_retries: 3,
        retry_backoff: Duration::from_millis(200),
    },
    wal, // Arc<dyn open_web_server_ledger::WriteAheadLog>
);

let receipt = ledger.commit(mutation_request).await?;
```

### 4.2 UDP-IP冗長経路を追加する(2026-07-11新設、任意)

```rust
use open_web_server_wire::udp_channel::UdpChannelKeys;

let keys = UdpChannelKeys::generate_for_testing(); // 本番はHKDF導出鍵を使う
let ledger = ledger
    .enable_udp_redundant_path(
        "0.0.0.0:0".parse()?,      // 送信元
        udp_dest_addr,              // 受信側 (open-runo等が実装する想定)
        &keys,
    )
    .await?;
```

呼び出さなければ従来通りTCP経路のみで動作する。**スコープの限界**:
UDP側は再送なしのfire-and-forget、受信側の実配置(open-runo側での
listener実装)は別スコープ。詳細は `docs/architecture.md`
「冗長化された伝送経路」節を参照。

### 4.3 QUIC冗長経路を追加する(2026-07-12新設、任意)

```rust
use open_web_server_wire::{QuicServer, QuicServerConfig, insecure_client_config_trusting, send_mutation_over_quic};

// サーバ側 (開発/検証用の自己署名証明書。本番は正規CA証明書に差し替え)
let config = QuicServerConfig::self_signed("example.internal")?;
let cert_der = config.cert_der.clone();
let server = QuicServer::bind("0.0.0.0:4433".parse()?, config)?;
let req = server.accept_one_mutation().await?; // 1接続=1 MutationRequestの単純往復

// クライアント側
let client_config = insecure_client_config_trusting(&cert_der)?;
let ack = send_mutation_over_quic(
    client_config,
    "0.0.0.0:0".parse()?,
    server_addr,
    "example.internal",
    &mutation_request,
).await?;
```

**スコープの限界**: Multipath QUIC(MPQUIC)ではなく単一経路QUIC。
1コネクション1双方向ストリームの単純往復のみサポート。受信側の実配置
(open-runo側での実運用listener)は別スコープ。

### 4.4 MPTCP/SCTP代替冗長経路(mptcp_channel)を追加する(2026-07-13新設、任意)

```rust
use open_web_server_wire::{MptcpServer, send_mutation_over_mptcp};

// サーバ側 (1接続=1 MutationRequestの単純往復)
let server = MptcpServer::bind_and_accept_one("0.0.0.0:4434".parse()?).await?;
let server_addr = server.local_addr();
let req = server.recv_one().await?;

// クライアント側
send_mutation_over_mptcp(server_addr, &mutation_request).await?;
```

**正直な注意(重要)**: これは本物のカーネルMPTCP/SCTPではない。
Windowsにはネイティブカーネル MPTCP が無く、主要な Rust SCTP クレートは
Linuxの`lksctp-tools`前提であるため、このWindows開発環境ではカーネル
MPTCP/SCTPの実装・検証が不可能と判断した(調査の詳細は
`mptcp_channel`モジュールdoc参照)。代わりに`aggligator`
(公式docで"serves the same purpose as Multipath TCP and SCTP...
completely implemented in user space"と明記)により、複数の物理TCPリンクを
1つの論理ストリームへ集約する**ユーザー空間の代替**を提供する。
**スコープの限界**: 1接続1メッセージの単純往復のみサポート。単一NIC
環境での検証のみ(複数物理NICでの真のマルチホーミング効果は未検証)。

### 4.5 PostgreSQL WALを使う(2026-07-12新設、任意)

```rust
use open_web_server_ledger::PostgresWal;
use std::sync::Arc;

let wal = PostgresWal::connect("postgres://user:pass@host/dbname").await?;
wal.ensure_schema().await?; // CREATE TABLE IF NOT EXISTS (冪等)

let ledger = Ledger::new(config, Arc::new(wal));
```

`append`/`mark_committed` はいずれも実`BEGIN`/`COMMIT`トランザクション
境界を持つ。**スコープの限界(正直な記載)**: このリポジトリの開発環境には
到達可能なライブPostgreSQLが無いため、実DB接続での検証は未実施
(SQL構築ロジックの単体テストと、`DATABASE_URL`設定時のみ動く
`#[ignore]`統合テストのみ提供)。導入先で実PostgreSQLに接続して
`cargo test -p open-web-server-ledger -- --ignored` を実行することで
実際のトランザクション動作を検証できる。

### 4.55 独立監査ログを使う(2026-07-13新設、任意)

```rust
use open_web_server_ledger::{Ledger, FileAuditLog};

let ledger = Ledger::new(config, wal)
    .enable_audit_log("/var/log/open-web-server/audit.log");

// commit() のたびに WAL先行書き込み直後、SHA-256チェックサム付きの
// 1レコードが追記される。権威パス(TCP経由3ホップコミット)には
// 一切影響しない (書き込み失敗は警告ログのみ)。

let audit = ledger.audit_log().unwrap();
let report = audit.reconcile(&committed_keys)?; // 突き合わせ
audit.scan_and_verify()?; // 破損検知(チェックサム再計算)
```

PostgreSQL/aruaru-db/マルチリージョン同期レプリケーションのいずれとも
技術的に独立した追記専用ファイル(`open-web-server-ledger::audit_log::
FileAuditLog`)。金融機関の「主系とは別システムの冗長トランザクション
ログによる二重処理検知」パターンの最小実装。

### 4.6 4層防御通信を単体で使う

```rust
use open_web_server_wire::{PayloadCipher, MutualAuthConfig, TlsServerConfig, SecureChannel};

// 第3層のみ(AEAD暗号化、リプレイ対策なし)
let cipher = PayloadCipher::new(&PayloadCipher::generate_key());
let encrypted = cipher.encrypt(b"payload")?;

// 第3層+第4層(AEAD暗号化 + seq/timestampリプレイ対策込み、
// 課金/決済確定など非冪等操作にはこちらを推奨)
let mut channel = SecureChannel::new(&PayloadCipher::generate_key());
let frame = channel.encrypt(b"charge:100yen")?;
```

## 5. 動作確認

```bash
cd open-web-server
cargo check --workspace
cargo test --workspace     # 18テストが全部通ればOK(gateway 4 / ledger 3 / wire 11)
cargo run -p open-web-server-gateway
```

`aruaru-db`/`open-runo`の実プロセスと結合した本物のエンドツーエンド起動には
それらのリポジトリの実行手順を参照(このリポジトリ単体にはMakefile/
docker-composeは無い)。

## 6. 命名規約(お引越し先でも守ること)

- クレート/ディレクトリ: `open-web-server-*` — Rustパス: `open_web_server_*`
- 環境変数: `OPEN_RUNO_ENDPOINT`(open-runoへの転送先エンドポイント)

## 7. 詳細ドキュメント

`CLAUDE.md`(方針・HANDOFF)/ `docs/architecture.md`(構成図・各層の説明)/
`docs/integration.md`(open-runo/aruaru-dbとの結合方法)。
