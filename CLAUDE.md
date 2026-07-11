# 技術スタック・開発ルール(open-web-server)

このリポジトリ、および関連プロジェクト(`open-runo`/`aruaru-db`/`aruaru-web`/
`open-raid-z`/`poem-cosmo-tauri`)で開発・保守を行う際は、以下を基本方針とする。
作業ドライブは `F:\open-runo`(E:ドライブは2026-07-10に消失、以後Fが実体)。
この節は [`open-raid-z`](https://github.com/aon-co-jp/open-raid-z) の
`CLAUDE.md` を正本とし、各プロジェクトへコピーして同期する
(最終同期: 2026-07-11、open-raid-z側の2026-07-10方針転換を反映)。

## 方針転換(2026-07-10、最終確定)

ユーザー指示により以下へ転換・確定。**Tauri・Poem・WunderGraph Cosmo(有料版
含む)を外部パッケージ/ライブラリとして直接依存させることはしない**。ただし
各ツールが提供する**機能・API形状・体験には互換性を保ち**、Rust標準ライブラリ
+ tokio/hyper で自前実装して置き換える(依存だけを断ち、機能面の互換性は
維持する)。**`poem-cosmo-tauri` と `open-runo` は2リポジトリを同時並行で
開発する**(2026-07-10、再確定)。どちらもTauri/Poemを含まない構成。
実装(例: crates/open-runo-routerのPoem→tokio/hyper移行)はpoem-cosmo-tauri
側で先行させ、動作確認できたファイルをopen-runoへミラーする運用とする。

**このリポジトリ固有の既知ギャップ**: `open-web-server-gateway` クレートは
本節執筆時点(2026-07-11)でまだ `poem`/`poem-openapi` パッケージに直接依存
したままで、tokio/hyper直接実装への移行は未着手。フロントエンド
(aruaru-web ネイティブUI相当)を持たないバックエンド専用リポジトリのため、
上記の「WASMフロントエンド」方針は直接は適用されないが、バックエンドの
Poem依存除去は他リポジトリと同じ方針に従うべき残作業として認識している。

**poem-cosmo-tauri と open-runo の違い(2026-07-11、ユーザー確認済み、
open-raid-z正本より転記)**: 両者は共通コア(Cosmo有料版機能のOSS Rust
再実装)を持つが**全く違うリポジトリのプロジェクト**であり統合対象では
ない。poem-cosmo-tauri はさらに範囲が広く、Poem/Tauriの**全機能を
AI駆動開発で一から自作・再現する**という上乗せ目標を持つ(open-runoには
ない)。詳細は open-raid-z の `CLAUDE.md` を参照。

## フロントエンド(2026-07-10、方針更新)

- Tauriパッケージには直接依存しない。ただしTauriのデスクトップUI体験・
  `invoke()`的なコマンド呼び出しインターフェースとは互換性を保つ。
- **HTML5/CSS3・TypeScript・Bootstrap・Node.jsのスタックは廃止**。
  Rustをメイン言語としてフロントエンドとバックエンドを統合し、
  **WebAssembly (WASM)** に置き換える(コンパイル対象はRust →
  `wasm32-unknown-unknown`)。DOM操作・`invoke()`相当の呼び出しは
  Rust製WASMモジュール側で行い、TypeScript/Node.jsのビルドチェーンには
  依存しない。https://webassembly.org/ | https://rustwasm.github.io/
- open-web-server自体はバックエンド専用リポジトリのため直接のWASM UIは
  持たないが、将来の管理画面(ロードマップ参照)はこの方針に従いRust/WASMで
  実装する(Tauriは使わない)。

## バックエンド・コア

- **Rust**(メイン言語、標準ライブラリ中心): https://www.rust-lang.org/ja/ | https://github.com/rust-lang/rust
- **tokio** + **hyper**(Webフレームワークなしで直接HTTPサーバを自前実装):
  https://tokio.rs/ | https://docs.rs/hyper/latest/hyper/
- Poemパッケージには依存しないが、Poemのルーティング/ハンドラAPI形状とは
  互換性のあるインターフェースを維持しながらtokio/hyper直接実装へ移行する
  (本リポジトリの `open-web-server-gateway` は上記「既知ギャップ」の通り
  この移行がまだ未実施)。

## このリポジトリ固有の役割

open-web-server は課金アイテム/金融データの消失防止に特化した Web サーバー。
`open-web-server-wire`(3層防御通信) → `open-web-server-ledger`(冪等WAL+3ホップ
コミット) → `open-runo`(Federation Gateway) → `aruaru-db`(分散Git-on-SQL)の
経路で、二重課金・データ消失を防ぐ。

### 拡張要件(2026-07-11、ユーザー指示——目標アーキテクチャ、実装は段階的に)

`open-web-server` と `poem-cosmo-tauri`(または `open-runo`)を用い、さらに
**PostgreSQL・`aruaru-db`・`open-raid-z`** を組み合わせて、3Dオンライン
ゲームの課金アイテム・金融データ・証券データ等が**ネットワーク上で紛失
しない**ことを目的とした以下の仕組みを実装する:

1. **VersionLessAPI(バージョンレス)とバージョン管理のハイブリッド**:
   エンドポイント自体はVersionLessAPI(open-runo/poem-cosmo-tauriのcosmo
   部分が既に提供)としつつ、データそのものは `aruaru-db`(Git-on-SQL、
   コミット単位の変更履歴)でバージョン管理する——APIはバージョンレス、
   データは完全な変更履歴を保持、というハイブリッド。
2. **Git管理**: `aruaru-db` の Git-on-SQL 特性を使い、課金アイテム/金融/
   証券データの全変更をコミットとして記録し、任意時点への復元・差分監査を
   可能にする。`open-raid-z`(RAID-Z2/Z3相当のディスク冗長化)をこの
   Git履歴・データ本体の永続化層の冗長化基盤として組み合わせる。
3. **TCP-IP・UDP-IPの三層三重通信**: 既存の `open-web-server-wire`
   (3層防御通信、現状TCPのみ)を拡張し、**TCP-IPとUDP-IPの両方**を使った
   三層・三重の冗長経路(例: 主系TCP + 副系TCP + UDP即時通知、またはTCP2系
   +UDP1系のいずれかの組み合わせ)で送受信することで、単一経路の障害・
   パケットロスがあってもデータ(特に課金・入出金トランザクション)が
   欠落しないようにする。UDPは低遅延の即時通知・ハートビート用途、TCPは
   確実な到達保証が必要な本体データ用途、という役割分担を基本方針とする。
4. **DATABASE二重(以上)書き込み**: PostgreSQL(整合性・トランザクション
   保証)と `aruaru-db`(Git履歴・分散耐障害性)の両方へ同一トランザクション
   を書き込み、`open-runo-db::DualBackend`(またはこのリポジトリ固有の
   同等機構)の整合性自動検証・自動修復パターンを踏襲する。

**実装方針**: 上記は目標アーキテクチャ全体像であり、一度に全て実装しようと
せず、段階的に検証可能な単位(例: まずUDP-IP経路を1本`open-web-server-wire`
に追加、次にaruaru-db連携のバージョン履歴確認、等)に分割して進めること。
各段階で実バイナリ・実ネットワーク通信による検証を行い、型チェックのみで
「完了」と報告しない。詳細な進捗は本ファイルのHANDOFF節に記録する。

**進捗(2026-07-11)**: 上記(3)のうちUDP-IP経路の**第一実装**が完了。
`open-web-server-wire::udp_channel` モジュールを新規追加し、
`open-web-server-ledger::Ledger` の commit パイプラインに
`enable_udp_redundant_path()` (任意有効化) として結線した。**スコープは
「UDP1系のみのfire-and-forget即時通知」に限定**しており、副系TCPの追加・
UDP側の再送機構は未実装(目標の「三層三重通信」全体にはまだ届いていない)。
詳細は本ファイルのHANDOFFと `docs/architecture.md` の該当節を参照。
(1)(2)(4)は引き続き未着手。

## API設計思想(参考・概念のみ)

- **VersionLess API**という考え方を参考にする(WunderGraphのブログ/podcast参照)。
- **WunderGraph Cosmo**: パッケージとしては直接依存させない。GraphQL
  Federation / VersionlessAPI というAPI形状・コンセプトのみ参考にし、
  Rust標準+tokio/hyperで互換性を保ちつつ自前実装する。
  https://github.com/wundergraph/cosmo

## 関連プロジェクト

- **open-runo**(poem-cosmo-tauriと同時並行開発。2026-07-10付けで開発再開):
  https://github.com/aon-co-jp/open-runo
- **open-web-server**(このリポジトリ): https://github.com/aon-co-jp/open-web-server
- **aruaru-db**: https://github.com/aon-co-jp/aruaru-db
- **aruaru-web**: https://github.com/aon-co-jp/aruaru-web
- **open-raid-z**(開発ルールの正本): https://github.com/aon-co-jp/open-raid-z
- **rs-to-readme**: https://github.com/aon-co-jp/rs-to-readme
- **poem-cosmo-tauri**(open-runoと同時並行開発。Poem→tokio/hyper移行の
  実装先行地点): https://github.com/aon-co-jp/poem-cosmo-tauri

## 運用ルール

- **開発中はこの`CLAUDE.md`を、コード変更のコミット/pushと必ず一緒に push する**。
- 実装で迷った場合は、学習データからの推測より公式ドキュメントを優先して参照する。
- 作業ドライブが変わった場合は、この節と関連プロジェクトの引き継ぎ資料を更新する。
- **無人自動開発(確認不要・自動デバッグ)のタイミングでは、スケジュール実行待ちに
  せず、1パス内でできる限り連続して作業を進める**こと。小さく検証可能な単位
  (1ハンドラ/1機能ごとに `cargo test` → commit → push)を保ちながらも、
  次の増分に進む前にバックグラウンド待機で止まらない。

## 現状(このリポジトリ固有)

- `cargo check --workspace` / `cargo test --workspace` は成功する(4クレート構成、
  2026-07-11時点で全13テストがgreen: gateway 4件 / ledger 3件 / wire 6件)。
- 4クレートの実装(`core`/`wire`/`auth`/`payload_crypto`/`tls`/`ledger`/`gateway`の
  各handler・middleware)はスタブなし。`todo!()`/`unimplemented!()`/`TODO`/`FIXME`は
  リポジトリ全体で0件(2026-07-11巡回時点でも再確認済み)。`handlers/wal.rs` の
  `InMemoryWal` は本番実装(sled/RocksDB/aruaru-db)への差し替え前提の参照実装で
  あることをdocコメントで明示済み — これは「隠れたスタブ」ではなく意図した設計。
- `open-web-server-gateway` に OpenTelemetry 連携(`src/telemetry.rs`)を追加済み
  (2026-07-11)。`grant_item`/`charge` ハンドラがスパン化され、
  `OTEL_EXPORTER_OTLP_ENDPOINT` の有無で OTLP/HTTP エクスポートと標準出力
  フォールバックを切り替える。テストはインメモリエクスポータで検証。

## HANDOFF (直近の自動巡回ログ、上が最新)

- **2026-07-11 (今回、UDP-IP冗長経路の第一実装)**: 拡張要件(3)「TCP-IP・
  UDP-IPの三層三重通信」のうち、UDP側の最初の具体的な実装を追加。
  **実装**: `crates/open-web-server-wire/src/udp_channel.rs` を新規作成。
  データグラム形式は `[seq:u64][HMAC-SHA256タグ:32B][PayloadCipher
  ciphertext(nonce12B+AEAD)]`。送信は `UdpSender::send_mutation`
  (fire-and-forget、再送なし)、受信は `UdpReceiver::recv_mutation`
  (HMAC検証→AEAD復号→`Deduplicator`によるIdempotencyKeyデデュープ)。
  `open-web-server-ledger::Ledger` に `enable_udp_redundant_path(bind_addr,
  dest, keys)` (任意呼び出しのasyncビルダー) を追加し、`commit()` の
  `fire_udp_redundant_notice()` が WAL先行書き込み直後に `tokio::spawn` で
  UDP送信を非同期発火、TCP経由の権威パス(既存の`forward_with_retry`)は
  一切ブロックしない設計。**スコープの限界(正直な記載)**:
  UDPの再送機構は実装せず「送りっぱなしの即時通知」のみ。副系TCP(TCP2系)は
  未実装。受信側の実配置(open-runo側でのUDP listenとaruaru-db WALとの結合)
  は別リポジトリのスコープで今回未着手 — 本リポジトリはUDP送信側の結線と、
  検証用の受信ロジック(`UdpReceiver`)の提供までに留まる。
  **テスト(実ソケット・実ネットワーク)**: `open-web-server-wire` に単体
  テスト6件を追加(フレームのround-trip、改ざん検知、鍵不一致拒否、
  デデュープ、および `tokio::net::UdpSocket` を `127.0.0.1:0` に実バインドし
  実送受信する結合テスト2件——1件は正常系の暗号化/HMAC/デデュープの実証、
  もう1件は未listenの宛先へ送ってもハング・panicしないことの実証)。
  `open-web-server-ledger` にも実TCPソケットのモックopen-runoサーバ
  (`tokio::net::TcpListener`、固定JSONレスポンス)を使った統合テスト2件を
  追加: (a) UDP経由の通知がTCP確定コミットと並行して正しく届きデデュープ
  される実証、(b) UDP宛先(`127.0.0.1:1`、未listen)が完全に到達不能でも
  TCP経由の権威パスは`tokio::time::timeout(5s)`内で問題なくコミット成功する
  実証(UDP障害がTCP経路を一切ブロック・破壊しないという設計保証の直接検証)。
  `cargo check --workspace` / `cargo test --workspace` は全13テストgreenを
  確認(gateway 4 / ledger 3 / wire 6)。
  **未実施(正直な記載)**: 実バイナリ(`open-web-server-gateway`等)の
  プロセス起動によるエンドツーエンド検証は、本リポジトリに
  Makefile/docker-compose等の実行手順が無く、`aruaru-db`/PostgreSQL相当の
  依存インフラもこの環境には無いため未実施。上記のクレートレベル実UDP
  ソケット統合テストをその代替とした。
  **ドキュメント**: `docs/architecture.md` に「冗長化された伝送経路」節を
  追加(3層防御通信=セキュリティレイヤーとの違いを明記)。
  `crates/open-web-server-wire/src/lib.rs` のモジュールdocに
  `udp_channel` との関係を追記。この`CLAUDE.md`の拡張要件(3)節に
  進捗ノートを追加(他3項目は未着手のまま明記)。
  **次回以降の候補**: 副系TCPの追加、UDP側の再送/ACK機構(必要性を再検討の
  うえ)、open-runo側でのUDP受信リスナー実装との結合。
- **2026-07-11**: git健全性を確認(壊れたref修復済み、`origin/main` の
  `2fd70a4` を正しく追跡、作業ツリークリーン)。`cargo check --workspace` /
  `cargo test --workspace --no-run` / `cargo test --workspace` すべて成功を再確認。
  `todo!()`/`unimplemented!()`/TODO/FIXME/stub/placeholder を再走査し0件を再確認。
  **実装**: `open-web-server-gateway` に OpenTelemetry 連携を追加。
  `crates/open-web-server-gateway/src/telemetry.rs` で `SdkTracerProvider` を構築し、
  `OTEL_EXPORTER_OTLP_ENDPOINT` が設定されていれば OTLP/HTTP (protobuf) へ、
  未設定なら `opentelemetry-stdout` で標準出力へスパンをエクスポートするよう
  切り替える構成にした。`tracing-opentelemetry` レイヤーを `tracing_subscriber`
  の `registry()` に `fmt` レイヤーと併せて登録し、`main.rs` はプロセス終了直前に
  `TelemetryGuard::shutdown()` でバッファをフラッシュする。`grant_item`
  (`handlers/items.rs`)・`charge`(`handlers/transactions.rs`)の各ハンドラに
  `#[tracing::instrument]` を追加(`#[handler]` の下に置く必要がある点に注意 —
  属性マクロは下から上へ適用されるため、`#[handler]` が先に素のasync fnではなく
  instrument後の関数を見るようにする)。動作検証用の単体テストを追加
  (`telemetry::tests::spans_are_recorded_and_exported_with_service_resource`):
  `opentelemetry_sdk` の `InMemorySpanExporter`(`testing` feature、dev-dependency)
  でネットワーク送信なしにスパン生成・Resource属性付与・エクスポートを検証。
  `cargo test --workspace` で新規テスト含め全件パス。
  **ドキュメント**: `docs/architecture.md` に「可観測性」節を追加。
  `README.md`(ルート)に4本目の柱として一言追記。`README-Japan.md`/
  `README-English.md`(この2つが唯一ロードマップ節を持つ詳細版)を更新し、
  OpenTelemetryのロードマップ項目を完了([x])にし、管理画面ロードマップ項目の
  「Tauri製」という古い表記を2026-07-10のスタック転換に合わせて「Rust→WASM製」
  に修正。他8言語版(`README-France.md`等)は元々ロードマップ節を持たない短縮版
  であり、今回の変更で不正確になる記述が無かったため変更していない。
  この`CLAUDE.md`のフロントエンド/バックエンド節をopen-raid-z側の2026-07-10
  最新版と同期し、関連プロジェクトに`poem-cosmo-tauri`を追加。
  **既知の残課題として明記**: `open-web-server-gateway` は依然として `poem`/
  `poem-openapi` に直接依存しており、tokio/hyper直接実装への移行(open-raid-z
  方針)はまだ未着手 — 次回以降の巡回候補として残す(スコープが大きいため
  今回は着手せず、GraphQLエンドポイント追加時にPoem脱却を併せて検討するのが
  効率的と判断)。
- **次回の巡回で見るべき点**: (1) `open-web-server-gateway` のPoem依存除去
  (tokio/hyper直接実装への移行、open-raid-z方針)。(2) GraphQLエンドポイント
  追加(`async-graphql` 等、Poem脱却と同時に検討すると手戻りが少ない)。
  (3) `open-cosmo` 共通クレート切り出しは、着手前に必ず `open-runo`/
  `aruaru-db` 側のCLAUDE.md HANDOFFログを確認し、互換性のある地ならしが
  既にあるかを確認すること(未確認のまま単独で着手しない)。(4) 管理画面
  (Rust→WASM、Tauriではない)。(5) OpenTelemetryは本リポジトリ側は実装済みだが、
  `open-runo`/`aruaru-db` 側が同じTrace Contextを伝播・エクスポートするように
  なって初めてエンドツーエンドのトレースになる — 両リポジトリの対応状況は
  今回未確認なので、次回以降に確認すること。
- 2026-07-10: ビルド/テストは既に green であることを確認
  (`cargo check --workspace` / `cargo test --workspace --no-run` / `cargo test --workspace`
  すべて成功)。リポジトリ全体を `todo!()`/`unimplemented!()`/TODO/FIXME/stub/placeholder
  で走査し、該当0件を確認(実装は完了しており追加のスタブ実装作業は無し)。
  **バグ発見・修正**: ルートの `README.md` が UTF-16LE(BOMなし)で保存されており、
  `file`コマンドで `data`(バイナリ扱い)と判定され、GitHub上で文字化けして表示される
  状態だった(10言語版の `README-*.md` は全てUTF-8で正常)。内容はそのままUTF-8に
  再保存して復旧。加えて、10言語版READMEの「他の言語」リンク行が
  日本語・英語版には無く、他8言語版は日本語/英語の2つしかリンクしていなかったため、
  全10ファイルで10言語すべてへの相互リンクに統一。CLAUDE.mdにこのHANDOFF節を追加。
  コミット・push済み(このコミットハッシュは `git log` 参照)。
- 2026-07-10: `open-web-server-ledger` がビルド不能だった問題を修正
  (Cargo.toml に `async-trait`/`chrono` の依存が抜けていた)。冪等性
  ショートサーキットの単体テストを追加(以前はこのクレートにテストが
  0件だった)。
