# open-web-server

**Rust + tokio/hyper 自前実装 Web サーバー — 課金アイテム・金融データを「消失させない」ために設計**

3D オンラインゲームの課金アイテム購入やクレジットカード決済のような、
ミッションクリティカルな 24/7/365 ワークロード向けの Web サーバーです。
`open-runo`(Federation Gateway)・`aruaru-db`(分散 Git-on-SQL DB)と
4 層防御通信で連携し、再送・プロセス再起動・ネットワーク瞬断があっても
二重課金やデータ消失が起きない設計になっています。

> 補足: ルーティング/ハンドラの API 形状は元の Poem 実装と互換性を保っていますが、
> パッケージとしては Poem に**依存しません**(2026-07-10 に tokio/hyper 直接実装へ移行済み)。

## 命名・関連リポジトリとの位置付け

**`open-web-server`** という名称は、ユーザーによって命名されました。
本リポジトリはクライアント向けの入口(REST API、冪等 WAL 先行書き込み)を
担当し、`open-runo`(または `poem-cosmo-tauri`、Federation Gateway)経由で
`aruaru-db`(分散 Git-on-SQL データベース)へ課金・金融データの確定を
届けます。

**`aruaru-server`** は `aruaru-db` リポジトリ内の実行バイナリクレート
(`aruaru-db/crates/aruaru-server`、PostgreSQL ワイヤプロトコル互換の
pgwire サーバー本体)で、こちらは開発の過程で Claude が名付けたもの
です。`aruaru-db` という分散データベースそのもののエンジン部分
(`aruaru-query`・`aruaru-wire`・`aruaru-dist` 等)を1つの起動可能な
サーバーとしてまとめた、`aruaru-db` エコシステムの「本体」に相当します
——`open-web-server`/`open-runo`/`poem-cosmo-tauri` から見ると、これら
すべてが最終的にデータを預ける先の永続化層です。

**別リポジトリとして切り出す必要はありません**——`aruaru-server` は
既に `aruaru-db` という単一の Cargo workspace 内の1クレート
(`[[bin]] name = "aruaru-server"`)として実装されており、`aruaru-query`
(SQL エンジン)・`aruaru-wire`(pgwire プロトコル)・`aruaru-dist`
(Raft分散合意)等、密結合した他クレートと同じ workspace で一緒に
ビルド・バージョン管理される設計になっています。分離すると、これらの
内部APIをクレート境界を越えて公開・安定化する必要が生じ、開発上の
メリットなくコストだけが増えます。

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
6. **目標アーキテクチャ: 通信層・DB書き込みの四重化**(2026-07-13更新) — 課金/金融/証券/クレジットカードデータをネットワーク上で失わないため、通信層は TCP-IP・UDP-IP・QUIC・MPTCP/SCTP相当の4方式、DB書き込みは PostgreSQL(ACID)・aruaru-db・マルチリージョン同期レプリケーション・独立監査ログの4系統を目標とする。**通信層4方式・DB書き込み4系統とも実装完了**: ①TCP-IP・②UDP-IP(既存)、③QUIC(`quic_channel`, `quinn`ベース、実TLS1.3+実UDPソケットで検証)、④MPTCP/SCTP代替(Windowsにはカーネル実装が無いため`aggligator`クレートによるユーザー空間代替、その旨明記の上で実ループバック検証済み)。①PostgreSQL WAL(`sqlx`ベース、ライブ接続はサンドボックスに無く未検証)、②aruaru-db×ZFSスナップショット連携(aruaru-db側で実装、実RAID-Z2プールで検証済み)、③マルチリージョン同期レプリケーション(`multi_region::MultiRegionReplicator`, 実SQLite2系統への同期書き込み+障害ポリシー選択、実I/Oで検証済み)、④独立監査ログ(`audit_log::FileAuditLog`, 実ファイルI/Oで検証済み)。残るは VersionLessAPI+Git版管理ハイブリッドの読み出し側クエリAPI(open-runoとの連携が必要)のみ(詳細は [README-Japan.md](README-Japan.md#6-目標アーキテクチャ-通信層dbの四重化) と [CLAUDE.md](CLAUDE.md#拡張要件2026-07-11ユーザー指示目標アーキテクチャ実装は段階的に))。
7. **静的ファイル + PHP配信**(`static_files`/`php_server`/`web_vhost`, 2026-07-20) — Apache+Nginxハイブリッド配信エンジンへの第一歩。ホスト名ごとにdocrootを割り当て、静的アセットは直接配信(パストラバーサル対策込み)、それ以外は`php -S`サブプロセスへのリバースプロキシで処理する。実際に`audiocafe.tokyo`(既存PHPサイト)を配信して検証済み(詳細は[README-Japan.md](README-Japan.md#7-静的ファイル--php配信apachenginxハイブリッド配信エンジンへの第一歩2026-07-20)参照)。
8. **IOWN/APN×Smart-TCPハイブリッド適応制御 + 圧縮/暗号化のハードウェアアクセラレータ抽象化**(2026-07-23) — [`RS-SmartTCP`](https://github.com/aon-co-jp/RS-SmartTCP)がRFC 6298/9002準拠のSRTT/RTTVAR EWMAで実測RTT/ジッターからネットワーク品質を判定し、IOWN/APNのような光ネットワーク級を検知した際にリトライ間隔等を積極化する(**正直な開示**: IOWN/APN自体はNTTが構築する物理telecom基盤であり本クレートが「実装」する対象ではない)。`open-web-server-wire::accel`は圧縮+暗号化をCPU(実装済み)/GPU/NPU/専用ハードウェアアクセラレータ(未実装の拡張点、安全にCPUへフォールバック)で切り替え可能にする抽象化。
9. **組み込みSFTPサーバー + UPnP自動ポート開放**(`sftp`/`upnp` feature、2026-07-23) — 固定IPを持たない自宅サーバー等でも外部の`sshd`に頼らずSFTP接続を受けられる(`russh`/`russh-sftp`、公開鍵認証・パストラバーサル対策込み)。UPnP IGD(`igd-next`)によるポート自動開放は明示opt-in(`OPEN_WEB_SERVER_UPNP_AUTO_FORWARD=true`)の補助機能で、失敗してもSFTPサーバー起動はブロックしない。`GET /admin/sftp/connection-info`で接続情報(ホスト・ポート・接続コマンド例)を1回のAPI呼び出しで確認できる。実SFTPクライアントでのmkdir/アップロード/一覧取得/ダウンロード/削除の往復まで実証済み(詳細は[PORTING.md §4.8](PORTING.md#48-組み込みsftpサーバー--upnp自動ポート開放2026-07-23新設任意)、[CLAUDE.md](CLAUDE.md)の同日HANDOFF参照)。
10. **無料DDNS(DuckDNS)自動ドメイン取得〜自動更新、最大20ドメイン対応**(`ddns` feature、2026-07-23) — 固定IPを持たない環境向けに、無料で・有効期限切れの心配無く使えるサブドメイン(DuckDNS、No-IPと異なり30日ごとの手動確認が不要)を`OPEN_WEB_SERVER_DUCKDNS_DOMAIN`/`OPEN_WEB_SERVER_DUCKDNS_TOKEN`の2環境変数だけで自動取得・自動更新する(`free_domain.rs`)。1インスタンスにつき最大20ドメインまで動的登録・自動更新でき(`tenant_router::TenantRegistry`と同じ`RwLock<HashMap<..>>`パターン、21件目以降は明示的な400エラーで拒否)、管理API`POST /admin/ddns/setup-free-domain`(複数回呼べば追加登録)・`GET /admin/ddns/domains`(一覧+残り枠)・`DELETE /admin/ddns/domains/:domain`(削除)で運用する。`GET /admin/sftp/connection-info`は生IPよりDuckDNSホスト名を優先して返し(`?host=`で複数ドメインから選択可)、「一度設定すれば変わらない」SFTP接続コマンドが得られる。**正直な開示**: DuckDNSアカウント自体の取得(トークン発行)はユーザー自身のOAuthログインが必要で自動化しない。**2026-07-23、実DuckDNSエンドポイント(`https://www.duckdns.org/update`)へダミーの無効トークンで実接続検証済み**: `HTTP 200`+プレーンテキストボディ`KO`(無効トークン時)が実際に返ることを確認し、`update_duckdns()`の`body.trim_start().starts_with("OK")`判定ロジックが実際のDuckDNS応答形式と一致していることを裏取りできた(実DuckDNSアカウント作成・有効トークンでの成功系E2Eは今回も未実施、ユーザー自身の作業として残る)。`open-easy-web`側に対応する一覧+追加フォーム形式のウィザードUIも追加済み(詳細は[PORTING.md §4.9](PORTING.md#49-無料ddnsduckdns自動ドメイン取得自動更新2026-07-23新設ddns-feature配下)、[CLAUDE.md](CLAUDE.md)の同日HANDOFF参照)。
11. **CORS対応**(`middleware::cors`、2026-07-23、既定無効・オプトイン) — `open-easy-web`のドメイン設定ウィザード(別オリジンのブラウザ上WASM)が管理API(`/admin/*`)を`fetch()`で叩けるようにする。`OPEN_WEB_SERVER_CORS_ALLOWED_ORIGINS`(カンマ区切り)未設定時はCORSヘッダーを一切付与せず既存動作を完全維持。設定時のみ、許可オリジンからの通常リクエストへ`Access-Control-Allow-Origin`等を付与し、`OPTIONS`プリフライトを`dispatch`より先に`204`で処理する(`x-admin-token`を含む`Access-Control-Allow-Headers`)。実HTTP経由の統合テスト2件(許可/拒否オリジンでのヘッダー有無、プリフライト応答)で検証済み。
12. **Android版(第一段階、着手・未完成)**(2026-07-23) — `android/`配下にKotlin製の最小限単一Activityシェルを新設。`cargo ndk`でクロスビルドした`open-web-server`実行ファイルを`libopenwebserver.so`としてリネームし`jniLibs/arm64-v8a/`に同梱、`nativeLibraryDir`(W^X制約下でも実行可能な領域)から`ProcessBuilder`で起動し、起動後に自分自身へ`GET /healthz`を投げて実際に応答することを画面上で確認する構成。3電源プロファイルUI・フォアグラウンドサービス化・署名/配布は今回のスコープ外(詳細・検証結果・正直な制限事項は[CLAUDE.md](CLAUDE.md)の同日HANDOFF参照)。

> ⚠️ **通信層4方式の正直な位置づけ(2026-07-23、日英Web検索で再検証)**: ①②③(TCP/UDP/QUIC)は決済業界の実務と整合するが、④(MPTCP/SCTP代替の`aggligator`)は「金融機関が実際に複数物理経路を冗長化する方法(SD-WAN等のネットワークインフラ層)」とはレイヤーが異なる、次善策としての位置づけ(詳細は[CLAUDE.md](CLAUDE.md)の同日エントリ参照)。

13. **構造化アクセスログ + サイズローテーション**(`access_log`、2026-07-24新設、既定無効・オプトイン) — Nginx/Apacheの運用ベストプラクティス(日英Web検索で確認)を参考に、JSON Lines形式の永続アクセスログを追加。`OPEN_WEB_SERVER_ACCESS_LOG_PATH`設定時のみ有効化し、`OPEN_WEB_SERVER_ACCESS_LOG_MAX_BYTES`(既定10MiB)超過で`.1.gz`へgzip圧縮ローテーション、`OPEN_WEB_SERVER_ACCESS_LOG_MAX_BACKUPS`(既定5)世代までシフト保持する。実バイナリでのローテーション+gzip展開まで実機検証済み(詳細は[PORTING.md §4.11](PORTING.md#411-構造化アクセスログローテーション2026-07-24新設既定無効オプトイン)参照)。
14. **RS-LinkFusion(WAN/LAN/WiFiボンディング)との連携を実機検証**(2026-07-24) — `open-web-server`は`OPEN_WEB_SERVER_BIND`でbindアドレスを外部注入するだけでネットワークインターフェースに関知しない設計のため、[RS-LinkFusion](https://github.com/aon-co-jp/RS-LinkFusion)のボンディングトンネル経由での動作に**追加のコード変更は不要**と実機検証(3プロセス・実TCPソケットでのcurl疎通)で確認した。TUN仮想アダプタ方式(`gateway-serve`/`gateway-connect`)は管理者権限が必要なためこの開発環境では未検証(詳細は[PORTING.md §4.12](PORTING.md#412-rs-linkfusionwanlanwifiボンディングとの連携2026-07-24実機検証済み追加コード不要)参照)。

## クイックスタート(5分で動かす)

3プロセスを別ターミナルで起動します(依存する順に①→②→③)。

```bash
# ① aruaru-db (分散 Git-on-SQL DB)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# ② open-runo (Federation Gateway)
cargo run -p open-runo-gateway

# ③ open-web-server (このリポジトリの本体。デフォルトで 0.0.0.0:8080 で待受)
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 \
  cargo run -p open-web-server-gateway
```

起動したら、別ターミナルから課金アイテムを1つ付与してみます
(`Idempotency-Key` ヘッダは必須 — 無いと `400 Bad Request` になります):

```bash
curl -X POST http://127.0.0.1:8080/api/v1/items/grant \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: 11111111-1111-1111-1111-111111111111" \
  -d '{
    "idempotency_key": "11111111-1111-1111-1111-111111111111",
    "account_id": "user-42",
    "item_id": "sword_of_flame",
    "quantity": 1
  }'
```

同じ `Idempotency-Key` で**もう一度同じリクエストを送っても**、アイテムが
二重に付与されることはありません(§0 のゼロロス使命そのものの動作確認)。
レスポンスの `db_commit_id` が非 null になっていれば、`aruaru-db` への
コミットまで完了しています。

決済(クレジットカード等)も同じ形で叩けます:

```bash
curl -X POST http://127.0.0.1:8080/api/v1/transactions/charge \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: 22222222-2222-2222-2222-222222222222" \
  -d '{
    "idempotency_key": "22222222-2222-2222-2222-222222222222",
    "account_id": "user-42",
    "amount_cents": 999,
    "currency": "JPY"
  }'
```

ヘルスチェック(認証不要): `curl http://127.0.0.1:8080/healthz`

環境変数: `OPEN_RUNO_ENDPOINT`(既定 `https://127.0.0.1:8443`)、
`OPEN_WEB_SERVER_BIND`(既定 `0.0.0.0:8080`)。

## 新しいエンドポイントを追加する(最小の例)

Web フレームワークを使わない自前実装なので、新しいエンドポイントの追加は
「① リクエスト/レスポンス型を定義 → ② ハンドラ関数を書く → ③ `main.rs` の
`dispatch()` に1行足す」の3ステップだけです。以下は在庫確認用の
`GET /api/v1/items/:account_id` を追加する最小例です。

```rust
// crates/open-web-server-gateway/src/handlers/items.rs に追記

use crate::response::json_response;

/// `GET /api/v1/items/status?account_id=user-42`
pub async fn item_status(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    // クエリパラメータなど任意のパース処理をここに書く
    let account_id = req
        .uri()
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("account_id=")))
        .unwrap_or_default()
        .to_string();

    // 実際の照会ロジックは state.ledger や独自のクエリ経由で実装する
    json_response(StatusCode::OK, &serde_json::json!({ "account_id": account_id }))
}
```

```rust
// crates/open-web-server-gateway/src/main.rs の dispatch() に1行追加

match (method, path.as_str()) {
    (Method::POST, "/api/v1/items/grant") => handlers::items::grant_item(state, req).await,
    (Method::GET, "/api/v1/items/status") => handlers::items::item_status(state, req).await, // ← 追加
    (Method::POST, "/api/v1/transactions/charge") => {
        handlers::transactions::charge(state, req).await
    }
    (Method::GET, "/healthz") => text_response(StatusCode::OK, "ok"),
    _ => text_response(StatusCode::NOT_FOUND, "not found"),
}
```

書き込み系(POST)のエンドポイントを追加する場合は、`middleware::idempotency::check`
の対象パス一覧(`crates/open-web-server-gateway/src/middleware/idempotency.rs` の
`needs_key` 判定)にパスのプレフィックスを追加するのを忘れないでください —
これを忘れると Idempotency-Key 必須化(§0 のゼロロス保証の要)が効かない
エンドポイントを作ってしまいます。

## 自己ホスト構成の方針(2026-07-21策定、2026-07-22完了・DONE)

`runo.tokyo/open-web-server`(紹介ページ)は当初nginxの`alias`による
静的ファイル直接配信で公開していたが、**それでは`open-web-server`自身が
Apache/Nginx相当のWebサーバーであることを実証できていない**という
指摘を受け、構成を切り替えた。**2026-07-22、実VPS上で完了・実HTTPS
検証済み**(詳細は`CLAUDE.md`のHANDOFF「2026-07-22」節参照)。

**実施内容**:

1. VPS上に`open-web-server`をデプロイし、`web_vhosts.toml`
   (`host="runo.tokyo"`, `docroot="/root/open-web-server/site"`,
   `php_enabled=false`)で紹介ページ(`site/index.html`)を配信させた
   (`OPEN_WEB_SERVER_WEB_VHOSTS_FILE`環境変数、詳細は
   `web_vhosts.toml.example`参照)。ポート`127.0.0.1:8103`
   (`OPEN_WEB_SERVER_BIND`)でsystemdサービス`open-web-server.service`
   として常駐。
2. **手前のリバースプロキシ設定について(当初方針からの実装上の判断
   変更)**: 当初は`open-easy-web`の`scripts/gen-vhost.sh --stack=proxy`
   で生成する方針だったが、実際に検証したところ同スクリプトは
   **ドメイン単位で新規vhostサーバーブロック一式を生成する**設計
   (`<DOMAIN> <BIND_IP> [UPSTREAM]`)であり、本件のように**既存の
   `runo.tokyo`サーバーブロック内の1つの`location`だけ**
   (`/rgit/`等と同居)を`alias`から`proxy_pass`に差し替えるという
   ユースケースには噛み合わない(ドメイン全体を再生成する形になり、
   同居している他の`location`定義を壊すリスクがある)と判断した。
   そのため、既に同じ`runo.tokyo`設定ファイルで実績のあるRGitと同じ
   パターン(`location /rgit/ { proxy_pass http://127.0.0.1:8090/; ... }`)
   に倣い、`/etc/nginx/conf.d/runo-tokyo-tls.conf`の443(SSL)サーバー
   ブロック内の`location /open-web-server/`を`alias`から`proxy_pass
   http://127.0.0.1:8103/`へ直接書き換えた(ポート80のリダイレクト
   専用ブロックではなくSSLブロックであることを事前に確認済み——RGit
   導入時に一度これを誤った既知の落とし穴があるため)。
3. **実証**: `https://runo.tokyo/open-web-server/`が実際に
   `open-web-server`プロセス経由で配信されていることを、`nginx -t`
   だけでなく実際に(a) 正常系で`curl`が200と`<title>open-web-server</title>`
   を返すこと、(b) `systemctl stop open-web-server`でプロセスを一時停止
   すると同URLが**502**に変わること(=静的aliasではなく本当にプロキシ
   していることの証明)、(c) `systemctl start`で再開すると200に戻ること、
   の3段階で確認した(単に`nginx -t`が通っただけで「完了」と報告しない
   よう徹底)。

進捗・実機検証結果は`CLAUDE.md`のHANDOFF節に記録する。

## 構成(4 クレート)

`open-web-server-core`(ドメイン型/エラー) ・ `open-web-server-wire`(4 層防御通信) ・
`open-web-server-ledger`(冪等 WAL + 3 ホップコミット) ・ `open-web-server-gateway`(tokio/hyper ゲートウェイ)。
詳細は [docs/architecture.md](docs/architecture.md) / [docs/integration.md](docs/integration.md)。

## License

Apache-2.0
