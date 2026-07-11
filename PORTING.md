# PORTING.md — open-web-server お引越しファイル

> このファイル1枚で、他プロジェクトへ `open-web-server` を導入・移設できます。
> 対象バージョン: workspace 0.1.0(4クレート / 18テスト、2026-07-11実測、
> open-web-server-wireを3層防御通信→4層防御通信へ拡張済み)。
> 最新のクレート数・テスト数は `CLAUDE.md` の「現状」節を参照。
> 最終更新: 2026-07-11

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
│   ├── open-web-server-wire/     ← 4層防御通信 + UDP-IP冗長経路(udp_channel)
│   ├── open-web-server-ledger/   ← 冪等WAL + 3ホップコミット
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

### 4.3 4層防御通信を単体で使う

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
