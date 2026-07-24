# PORTING.md — open-web-server お引越しファイル

> このファイル1枚で、他プロジェクトへ `open-web-server` を導入・移設できます。
> 対象バージョン: workspace 0.1.0(4クレート。2026-07-20時点でgateway 46 /
> ledger 20[+1件は要ライブPostgreSQLの`#[ignore]`] / wire 18の計84テスト
> green。open-web-server-wireにQUIC(`quinn`)伝送路、MPTCP/SCTPのユーザー
> 空間代替(`aggligator`)伝送路、open-web-server-ledgerにPostgreSQL WAL
> [`sqlx`]、open-web-server-gatewayに静的ファイル+PHP配信[§4.7]を追加)。
> 最新のクレート数・テスト数は `CLAUDE.md` の「現状」節を参照
> (この節自体も更新のたび古くなるため、都度そちらを正とする)。
> 最終更新: 2026-07-20

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
(2026-07-20時点で84テストが通れば移設成功)。以下はライブラリとして個別に使う場合。

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

### 4.56 マルチリージョン同期レプリケーションを使う(2026-07-13新設、任意)

```rust
use open_web_server_ledger::{Ledger, MultiRegionReplicator, RegionPolicy};

let regions = MultiRegionReplicator::new(vec![
    "sqlite:///data/region-tokyo.db",
    "sqlite:///data/region-osaka.db",
])
.with_policy(RegionPolicy::Quorum(1)) // 2系統中1系統成功でOK(縮退モード)
.await?;
// デフォルト(RegionPolicy::Strict)は1系統でも失敗すればコミット全体を失敗させる

let ledger = Ledger::new(config, wal).enable_multi_region(regions);

// commit() は全リージョンへの書き込みが完了する(または縮退条件を満たす)まで
// 待ってから成功を返す — UDP冗長経路のfire-and-forgetとは対照的な
// 「同期」レプリケーション。
```

本番では2つの`sqlite:///...`パスを実際の地理的リージョンのDB接続
文字列(PostgreSQL等)に置き換える想定。障害ポリシーは用途に応じて
`Strict`(既定、厳格)か`Quorum(n)`(縮退許容)を選択する。

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

### 4.7 静的ファイル/PHPサイト配信(Apache+Nginxハイブリッド配信エンジン、2026-07-20)

`open-web-server-gateway`の新規モジュール`static_files`/`php_server`/
`web_vhost`は、既存のAPIバックエンド用途(`tenant_router`)とは独立した
「ホスト名 → docroot」のvhost機構で、静的ファイル配信とPHP実行
(PHPビルトインdevサーバへのリバースプロキシ)の両方を提供する。
これは他プロジェクトへの移植価値が高い(PHP資産を持つ他のドメイン
[例: aruaru.tokyo/PHP版、その他のレガシーPHPサイト]をopen-web-server配下
へ統合する際にそのまま使える):

```toml
# web_vhosts.toml
[[webvhost]]
host = "example.com"
docroot = "/var/www/example"
php_enabled = true
```

```bash
OPEN_WEB_SERVER_WEB_VHOSTS_FILE=./web_vhosts.toml ./open-web-server
```

管理APIでの動的追加/削除も可能: `POST /admin/web-vhosts` /
`DELETE /admin/web-vhosts/:host` / `GET /admin/web-vhosts`
(既存の`x-admin-token`/`KeyGuardian`認証を共用)。

**移植時の注意**: `php_server::PhpServerPool`はデフォルトで
`OPEN_WEB_SERVER_PHP_BINARY`環境変数(未設定時はこの開発環境固有の
WinGet配布パス)からPHP実行ファイルを探す — 他環境へ移植する際は必ず
この環境変数を設定すること。本番運用では`php -S`(開発用ビルトイン
サーバ)ではなくPHP-FPM + FastCGIへの置き換えを推奨(`php_server.rs`
のモジュールdocに明記)。

### 4.8 組み込みSFTPサーバー + UPnP自動ポート開放(2026-07-23新設、任意)

固定IPを持たない自宅サーバー等でも、外部の`sshd`/`vsftpd`に頼らず
`open-web-server`単体でSFTP接続を受けられるようにする2つの独立した
opt-in機能。既定は両方オフ(単一バイナリでの完結・オプトイン設計という
既存方針を踏襲)。

```toml
# Cargo.toml
open-web-server-gateway = { ..., features = ["sftp", "upnp"] }
```

```bash
# authorized_keys(OpenSSH形式)を用意した上で起動
OPEN_WEB_SERVER_SFTP_BIND=0.0.0.0:2222 \
OPEN_WEB_SERVER_SFTP_ROOT=./sftp-root \
OPEN_WEB_SERVER_SFTP_AUTHORIZED_KEYS_FILE=./authorized_keys \
OPEN_WEB_SERVER_UPNP_AUTO_FORWARD=true \
./open-web-server
```

- `sftp.rs` — `russh` + `russh-sftp`(pure-Rust)によるSFTPサーバー本体。
  公開鍵認証が基本(パスワード認証は`OPEN_WEB_SERVER_SFTP_ALLOW_PASSWORD_AUTH=true`
  + `OPEN_WEB_SERVER_SFTP_PASSWORD`で明示opt-in)。`OPEN_WEB_SERVER_SFTP_ROOT`
  配下へのパストラバーサル対策は`static_files.rs`と同じ
  canonicalize + starts_this方針。
- `upnp.rs` — `igd-next`によるUPnP IGD自動ポート開放の**補助機能**。
  失敗してもSFTPサーバー起動自体はブロックしない(`ddns.rs`/`acme.rs`と
  同じ「補助系の失敗は権威パスをブロックしない」設計)。UPnP非対応
  ルーターでは失敗する旨を正直に`tracing::warn!`で案内する。
- `GET /admin/sftp/connection-info` — 既存の`x-admin-token`/`KeyGuardian`
  認証で保護された、接続情報確認ヘルパー(ホスト・ポート・
  `sftp -P <port> user@<host>`形式のコマンド例をJSONで返す)。

**移植時の注意**: `authorized_keys`ファイルは他の多くのSSHサーバーと同じ
OpenSSH形式(`ssh-ed25519 AAAA... comment`)がそのまま使える。UPnPは
実ルーターの無い開発環境では実機検証できない(このリポジトリの検証も
API呼び出しの型・ロジックレベルに留まる、正直な開示は`upnp.rs`の
モジュールdoc参照)。

### 4.9 無料DDNS(DuckDNS)自動ドメイン取得〜自動更新(2026-07-23新設、`ddns` feature配下)

固定IPを持たない環境向けに、**無料で・有効期限切れの心配無く**使える
サブドメインを本ソフトウェア側で完結して運用する機能。第一候補として
DuckDNS(https://www.duckdns.org/)を採用した理由(裏取り込み):

- 無料、更新APIは`GET`リクエスト1本(`https://www.duckdns.org/update?
  domains=<name>&token=<token>&ip=<ip>`)。
- **有効期限切れの概念が無い**——No-IP無料プランのような「30日ごとに
  メール内リンクを手動クリックしないと失効する」制約が無いため、今回の
  「自動更新で永久に使える」要件に合致する(No-IPはこの理由で候補から
  除外した)。

```bash
OPEN_WEB_SERVER_DUCKDNS_DOMAIN=myhost \
OPEN_WEB_SERVER_DUCKDNS_TOKEN=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx \
./open-web-server
```

上記2環境変数のみで、既存`ddns.rs`と同じ「5分間隔でグローバルIP変化を
検知し自動更新」ループが起動する(`free_domain.rs`)。既存の汎用URL
テンプレート方式(`OPEN_WEB_SERVER_DDNS_UPDATE_URL`)とは独立して併存
可能。

#### 複数ドメイン対応(2026-07-23追記、最大20ドメイン)

1インスタンスにつき最大`free_domain::MAX_DUCKDNS_DOMAINS`(=20)件まで、
DuckDNSサブドメインを動的に登録・自動更新できる。設計は既存の
`tenant_router::TenantRegistry`と同じ`RwLock<HashMap<..>>`パターン
(`free_domain::DomainRegistry`)。上記の環境変数は1件目のドメインとして
起動時にシードされる(後方互換)。

- `POST /admin/ddns/setup-free-domain`(`x-admin-token`認証、
  `{"domain": "myhost", "token": "..."}`)は**複数回呼べば複数ドメインを
  追加登録**できる。1回の呼び出しは1ドメインの登録+即時疎通確認。
  21件目以降は`400 Bad Request`+理由付きメッセージで明示的に拒否される
  (無言で失敗しない)。
- `GET /admin/ddns/domains` — 登録済みドメイン一覧+残り登録可能件数。
- `DELETE /admin/ddns/domains/:domain` — 登録解除(自動更新ループの対象
  から即座に外れる)。

**正直な開示**: DuckDNSアカウント自体(トークン発行)はduckdns.orgへの
ユーザー自身のOAuthログインが必要で、これは本ソフトウェアから自動化
しない(他社サービスの認証情報を代行取得しない既存方針)。またこの
APIは登録+疎通確認のみを行い、環境変数自体の永続化(設定ファイル
書き込み等)は行わない——恒久的な自動更新には上記環境変数を設定した
上での再起動が必要(ただし動的登録経由のドメインはプロセス再起動不要で
即座に自動更新ループへ組み込まれる)。

`GET /admin/sftp/connection-info`は、登録済みDuckDNSドメインがあれば
(`OPEN_WEB_SERVER_SFTP_PUBLIC_HOST`の次に)その場で取得した生グローバル
IPより優先してホスト名として使う——DDNSで確保した「一度設定すれば
変わらない」永続ホスト名の方が、固定IPが無い環境のSFTP接続コマンドと
して実用上ずっと有用なため。複数ドメイン登録時は`?host=<domain>`
クエリパラメータでどのドメインを使うか選べる(未指定時は登録済み
ドメインのうち辞書順先頭が既定値)。レスポンスの
`available_duckdns_domains`で登録済み全ドメインのフルホスト名一覧を
確認できる。

**移植時の注意(2026-07-23更新)**: 実DuckDNSエンドポイント
(`https://www.duckdns.org/update`)へダミーの無効トークンで実接続
検証**済み**——`HTTP 200`+プレーンテキストボディ`KO`が実際に返る
ことを確認し、`update_duckdns()`の`body.trim_start().starts_with("OK")`
判定ロジックが実際のDuckDNSの応答形式(成功時`OK`、失敗時`KO`)と
一致していることを裏取りできた。ただし**実DuckDNSアカウント作成・
有効トークンでの成功系E2E検証は今回も未実施**(他社サービスの
認証情報を代行取得しない既存方針により、ユーザー自身の作業として
残る)。20件の上限は実DuckDNSサービス側の制約ではなく、本ソフトウェア
独自の運用上限(マジックナンバー化を避けた定数`MAX_DUCKDNS_DOMAINS`)
である点に注意。

### 4.10 CORS(Cross-Origin Resource Sharing)対応(2026-07-23新設、既定無効・オプトイン)

`open-easy-web`のようなブラウザ上WASMフロントエンドが、別オリジン
(別ポート/別ホスト)から本サーバーの管理API(`/admin/*`)を`fetch()`で
叩けるようにするミドルウェア。**既定で無効**——環境変数
`OPEN_WEB_SERVER_CORS_ALLOWED_ORIGINS`が未設定ならCORSヘッダーを
一切付与せず、既存動作を完全に維持する(オプトイン方式)。

```bash
# カンマ区切りで複数オリジンを許可できる
OPEN_WEB_SERVER_CORS_ALLOWED_ORIGINS="http://localhost:8080,https://wizard.example.com" \
./open-web-server
```

有効化すると、`middleware::cors`が`main.rs`の`route()`関数内で以下を
行う:

- **プリフライト(`OPTIONS` + `Access-Control-Request-Method`)**:
  許可オリジンからのものであればルーティング(`dispatch`)より先に
  `204 No Content`で即応答し、`Access-Control-Allow-Origin`/
  `-Methods`(`GET, POST, PUT, DELETE, OPTIONS`)/`-Headers`
  (`content-type, x-admin-token, authorization, idempotency-key`——
  管理APIの`x-admin-token`を含む)を付与する。許可されていないオリジンの
  プリフライトには何も付与せず`dispatch`へ素通りする(通常のルーティング
  結果、多くの場合`404`または`405`になる)。
- **通常リクエスト**: `dispatch`完了後(圧縮処理と同じ位置)に、リクエストの
  `Origin`ヘッダーが許可リストに含まれる場合のみ同じCORSヘッダーを
  レスポンスへ追加する。許可されていないオリジンにはヘッダーを一切
  付けない(ブラウザ側のCORS enforcementに委ねる設計であり、サーバー側で
  リクエスト自体を拒否するものではない——一般的なCORS実装と同じ)。

**移植時の注意**: `open-easy-web`側は追加のコード変更不要(ブラウザの
`fetch()`が標準のCORSプロトコルに従うだけ)。実HTTP経由の統合テスト2件
(`main.rs`の`cors_headers_and_preflight_work_over_real_http`/
`cors_headers_are_absent_by_default_over_real_http`)で、許可/拒否
オリジンでのヘッダー有無・プリフライト応答・既定無効時の非干渉を
検証済み。

### 4.11 構造化アクセスログ+ローテーション(2026-07-24新設、既定無効・オプトイン)

商用Webサーバー(Nginx/Apache)の運用ベストプラクティス(日英Web検索で
確認: JSON構造化ログ+サイズ/日付ベースのローテーション+圧縮保持)を
参考に、監査・分析用途の永続アクセスログを追加。既存の`tracing`ベースの
リクエストログ(標準出力/OTLP向け、開発者用途)とは独立して並存する。

```bash
OPEN_WEB_SERVER_ACCESS_LOG_PATH=/var/log/open-web-server/access.log \
OPEN_WEB_SERVER_ACCESS_LOG_MAX_BYTES=10485760 \
OPEN_WEB_SERVER_ACCESS_LOG_MAX_BACKUPS=5 \
./open-web-server
```

- `OPEN_WEB_SERVER_ACCESS_LOG_PATH`未設定なら完全に無効(ファイルI/Oを
  一切行わない、既定動作に影響なし)。
- 1行1リクエストのJSON Lines
  (`{"ts":"2026-07-24T...","method":"GET","path":"/healthz","status":200,
  "elapsed_ms":3,"remote_addr":"127.0.0.1:12345"}`)。
- ファイルサイズが`OPEN_WEB_SERVER_ACCESS_LOG_MAX_BYTES`(既定10MiB)を
  超えると、`access.log`を`access.log.1.gz`へgzip圧縮しつつローテートし、
  既存の`.1.gz`〜`.(N-1).gz`は`.2.gz`〜`.N.gz`へ世代シフトする
  (`OPEN_WEB_SERVER_ACCESS_LOG_MAX_BACKUPS`、既定5世代)。
- ファイルI/Oは`tokio::task::spawn_blocking`へ退避、書き込み失敗は
  リクエスト処理自体をブロックしない(`FileAuditLog`と同じ設計方針)。

**移植時の注意**: 新規クレート依存は不要(`flate2`/`serde_json`は既存の
`compression.rs`/API層で既に使用済み)。`crates/open-web-server-gateway/
src/access_log.rs`単体で持ち出せる(`AppState.access_logger`フィールドと
`main.rs`の`route()`/`accept_loop`/`accept_tls_loop`3箇所への配線が必要)。

### 4.12 RS-LinkFusion(WAN/LAN/WiFiボンディング)との連携(2026-07-24、実機検証済み・追加コード不要)

同一PCに`open-web-server`と`RS-LinkFusion`(`https://github.com/aon-co-jp/
RS-LinkFusion`)を両方インストールし、複数回線をボンディングした上で
Webサーバーを動かすシナリオは、**`open-web-server`側の追加実装なしで
既に機能する**ことを実機検証済み(`open-web-server`はbindアドレスを
環境変数で受け取るだけでネットワークインターフェースに関知しない設計
のため)。

```bash
# 1) open-web-server をローカルの任意アドレスで起動
OPEN_WEB_SERVER_BIND=127.0.0.1:18099 ./open-web-server

# 2) RS-LinkFusion のボンディング受け口(TCPポートフォワードモード)
rs-linkfusion serve --bind 127.0.0.1:15900 --target 127.0.0.1:18099 --key <hex鍵>

# 3) RS-LinkFusion のボンディング接続元
rs-linkfusion connect --listen 127.0.0.1:15199 --remote 127.0.0.1 --remote-port 15900 --key <hex鍵>

# 4) ボンディング経由でopen-web-serverへ到達することを確認
curl http://127.0.0.1:15199/healthz   # => "ok" (200)
```

**TUN仮想アダプタ方式(`gateway-serve`/`gateway-connect`、OSレベルの
全トラフィックをボンディングする本命シナリオ)についても設計上は同様に
動くはずだが、Windows実機での`wintun.dll`+管理者権限が必要なため今回の
開発環境(非管理者権限)では未検証**——`OPEN_WEB_SERVER_BIND`をTUN
仮想アダプタのIP(RS-LinkFusion既定`10.66.0.2`等)に向けるだけで動作する
想定。詳細は`RS-LinkFusion/PORTING.md`側にも追記済み。

### 4.11 Android版(`android/`、2026-07-23〜24新設、3電源プロファイル対応)

`android/`配下のKotlin/Gradleプロジェクトは、`open-web-server-gateway`
本体をクロスコンパイルして起動するだけの単一Activityシェル。移植先で
このAndroid版を使う場合の手順:

```bash
# 1) 対象ABIを両方クロスビルド(実機arm64-v8a + このマシンのx86_64 AVD)
cargo ndk -t arm64-v8a build --release --bin open-web-server
cargo ndk -t x86_64-linux-android build --release --bin open-web-server

# 2) それぞれ`lib<任意名>.so`にリネームしてjniLibsへ配置
#    (実行ファイルを.soの皮を被せてnativeLibraryDir配下に置く、という
#    Termux等が使う既知の手法——`assets/`直下は最近のAndroidのW^X制約下
#    では実行できないため)
cp target/aarch64-linux-android/release/open-web-server \
   android/app/src/main/jniLibs/arm64-v8a/libopenwebserver.so
cp target/x86_64-linux-android/release/open-web-server \
   android/app/src/main/jniLibs/x86_64/libopenwebserver.so

# 3) ビルド(Windows、gradlewを使わずキャッシュ済みgradle配布物を直接叩く例)
gradle assembleDebug --no-daemon
```

**実機能で踏んだ2つの罠(移植先でも同じことが起きるので明記)**:
1. **ABI不一致**: `jniLibs`に対象デバイス/エミュレータのABIのバイナリが
   無いと、`nativeLibraryDir`に何も展開されず`ProcessBuilder`の起動が
   `binary exists: false`で失敗する。実機スマホ/タブレットは
   `arm64-v8a`が主流だが、x86_64エミュレータで検証する場合は
   `x86_64`も追加で必要。
2. **ネイティブライブラリが展開されない(Android 6.0+既定)**:
   `build.gradle.kts`に`packaging { jniLibs { useLegacyPackaging =
   true } }`が無いと、ネイティブライブラリはAPK内から直接実行され
   (`status=run-from-apk`)、`nativeLibraryDir`ディレクトリ自体には
   一切展開されない。`ProcessBuilder`に実ファイルパスを渡す必要がある
   本アプリの構成では必須の設定。

**3電源プロファイル**(`PowerProfile.kt`/`ProfileSelectActivity.kt`):
🔋省電力/⚖️通常はどちらも`WakeLock`を取得しない(Android標準の
Doze/App Standbyに逆らわない、というのがそのまま「省電力対応」の実体)。
🔌常時電源接続のみ`PARTIAL_WAKE_LOCK`を保持する(`WAKE_LOCK`権限が
必要)。ホーム画面には3つの専用アイコン(`activity-alias`、色分け+
ラベル文字列でプロファイル名を明示)も用意しており、`ProfileSelect
Activity`(起動時選択画面)を経由せずアイコンから直接そのプロファイルで
起動できる。

**adbの`unauthorized`問題を踏んだ場合の対処**: ヘッドレス
(`-no-window`)でエミュレータを起動すると`adb devices`が
`unauthorized`のまま解消しないことがある(この開発環境で実際に発生)。
GUI操作可能な環境であれば、`-no-window`を外してウィンドウ付きで起動し、
完全ブートまで待つ(明示的な承認ダイアログのタップは不要だった)ことで
解消した。

**正直な制限事項**: 実機(物理スマホ/タブレット)での検証は未実施
(x86_64エミュレータでのみ実証済み)。APK署名・Google Play配布・
フォアグラウンドサービス化・Doze中のネットワークI/O制限自体の回避は
スコープ外のまま。

## 5. 動作確認

```bash
cd open-web-server
cargo check --workspace
cargo test --workspace     # 2026-07-20時点で84テストが全部通ればOK(gateway 46 /
                           # ledger 20[+1 ignored、要ライブPostgreSQL] / wire 18)
                           # 最新の内訳は CLAUDE.md の「現状」節を参照
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

## 8. 移植・拡張時の注意

他プロジェクトへ移植・拡張する際、依存クレートの現状(バージョン・
メンテナンス状況)や仕様の詳細で判断に迷う場合は、学習データからの
推測だけに頼らず、実際に検索(日本語・英語の両方のGoogle検索、および
GitHub上の実装状況調査)して裏付けを取ってから技術選定を行うことを
推奨する。

**パフォーマンス・並行処理**: 移植先でも、非同期(tokioマルチスレッド
ランタイム)を基本としつつ、必要な場面(CPU負荷の高い計算・厳密な順序
保証が必要な処理等)のみ同期処理を用いる方針を踏襲すること。
`#[tokio::main]`のランタイムflavorが`current_thread`に固定されて
いないか、async関数内でブロッキングI/O・CPU負荷処理を直接呼んでいないか
(`tokio::task::spawn_blocking`へ退避すべき)、CPU律速な処理は`rayon`等
でのデータ並列化を検討する価値があるか、を移植時にも確認するとよい。
4層4重の通信・DB冗長化アーキテクチャ(本ファイル§4参照)を移植する際も、
この並行処理方針は維持すること。

**最先端追従の方針(2026-07-23追記)**: 移植時の日英Web検索で
CockroachDB/TiKV等の最先端の実運用システムが既に対応済みと判明した
設計ギャップが見つかった場合、「今のところは大丈夫」という報告に
留めず、確認を求めず自動でそのまま実装に着手すること(ユーザー指示、
正本は`open-raid-z/CLAUDE.md`・`PORTING.md`同日エントリ参照)。

**ハードウェア非依存API + 安全なフォールバックパターン(移植元:
`open-web-server-wire::accel`、2026-07-23新設)**: 将来対応予定の
ハードウェア/バックエンド(GPU/NPU/専用アクセラレータ等)を`enum`の
選択肢として先に定義し、未実装のものが選ばれてもpanicせず既定実装
(CPU)へ安全にフォールバックしつつ`tracing::warn!`で可視化する。
呼び出し側のAPIは将来ハードウェアが実装された時と同じ形のまま——
実装が追いつくまでCPUが肩代わりする。存在しない能力を実装済みと
偽らないための必須パターン。

```rust
pub enum AccelBackend { Cpu, Gpu, Npu, HardwareAccelerator }

impl PayloadAccelerator {
    pub fn new(backend: AccelBackend, cipher: PayloadCipher) -> Self {
        let effective = match backend {
            AccelBackend::Cpu => AccelBackend::Cpu,
            other => { tracing::warn!(?other, "not implemented, falling back to Cpu"); AccelBackend::Cpu }
        };
        Self { backend: effective, cipher }
    }
}
```

**IOWN/APN×Smart-TCPハイブリッド適応制御**: [RS-SmartTCP](https://github.com/aon-co-jp/RS-SmartTCP)
参照(RFC 6298/9002準拠のSRTT/RTTVAR EWMA、独立リポジトリとして移植
可能)。

**Apache/Tomcat互換の多言語アプリケーションサーバー対応(2026-07-23、
ユーザー指示、正本は`open-raid-z/CLAUDE.md`同名節参照)**: このリポジトリ
(`app_proxy`/`tenant_router`)を他プロジェクトへ移植する際、「JavaのApache
のように動作し、Ruby on Rails/PHP+Laravel/Python+FastAPI等、言語を問わず
バックエンドを指せる」という設計を維持すること。転送先はプレーンHTTPの
ため、特定言語向けの専用実装(例: PHP専用の特別なプロキシロジック)を
追加しないこと——`backend_addr`が単体でHTTPサーバーとして応答しさえ
すれば、`TenantConfig`への登録だけで任意の言語・フレームワークを
ホストできる、という汎用性こそがこのモジュールの価値。移植先で
PHP-FPM/FastCGI等の本番グレード直結経路を追加する場合も、この
汎用リバースプロキシ経路とは独立したオプトイン機能として実装し、
既存の言語非依存パスを壊さないこと。
