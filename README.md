# open-web-server

**Rust + tokio/hyper 自前実装 Web サーバー — 課金アイテム・金融データを「消失させない」ために設計**

3D オンラインゲームの課金アイテム購入やクレジットカード決済のような、
ミッションクリティカルな 24/7/365 ワークロード向けの Web サーバーです。
`open-runo`(Federation Gateway)・`aruaru-db`(分散 Git-on-SQL DB)と
4 層防御通信で連携し、再送・プロセス再起動・ネットワーク瞬断があっても
二重課金やデータ消失が起きない設計になっています。

> 補足: ルーティング/ハンドラの API 形状は元の Poem 実装と互換性を保っていますが、
> パッケージとしては Poem に**依存しません**(2026-07-10 に tokio/hyper 直接実装へ移行済み)。

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

## 構成(4 クレート)

`open-web-server-core`(ドメイン型/エラー) ・ `open-web-server-wire`(4 層防御通信) ・
`open-web-server-ledger`(冪等 WAL + 3 ホップコミット) ・ `open-web-server-gateway`(tokio/hyper ゲートウェイ)。
詳細は [docs/architecture.md](docs/architecture.md) / [docs/integration.md](docs/integration.md)。

## License

Apache-2.0
