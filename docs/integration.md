# aruaru-db / open-runo / open-web-server 連携ガイド

## 役割分担

| プロジェクト | 役割 | 技術スタック |
|---|---|---|
| **open-web-server** | クライアント向け入口。REST/GraphQL API、課金・決済のWAL先行書き込み | Rust + tokio/hyper(Poem非依存) |
| **open-runo** | Graph Federation Gateway。認証・Rate Limit・監査・AIルーティングの一元管理 | Rust + tokio/hyper(Poem非依存) |
| **aruaru-db** | 分散 Git-on-SQL データベース。Raftによる強整合コミット、Git的な変更履歴 | Rust |

## 連携インターフェース

- open-web-server → open-runo(書き込み): `POST /internal/db/mutate` (JSON, `MutationRequest`)
- open-runo → aruaru-db: `aruaru-wire` (pgwire互換) 経由でSQL実行、または
  `aruaru-graphql` 経由でGraphQL Mutation実行
- 応答は必ず `MutationReceipt { idempotency_key, committed, db_commit_id, committed_at }`
  の形で返し、`db_commit_id` が入って初めて「確定」とみなす
- **open-web-server → open-runo(読み出し、2026-07-14実装)**:
  `GET /internal/db/state/:target/:key/at/:commit_id` — VersionLessAPI +
  Git-on-SQLハイブリッドの読み出し側。拡張要件(1)がこれまで書き込み側
  (`db_commit_id`の配線)のみ実質完成しており、「commit_idを指定して
  過去状態を問い合わせる」読み出し側が`open-web-server`に一切存在
  しなかったギャップを解消する。内部で
  `open-runo`の`GET /api/db/:table/:key/at/:commit_id`(`aruaru-db`
  バックエンドのみ実対応、他バックエンドは501)へプロキシする
  (`open-web-server-ledger::DbStateReader`)。認証は
  `POST /api/keys/self-issue`による自動キー発行+キャッシュ+期限切れ時
  の透過的再発行(CLIやWASMフロントエンドと同じ「人間がAPIキーを
  意識しない」方針)。応答が無い場合(`404`)は`Ok(None)`として扱われ
  (コミット不明、またはその時点でキー未存在)、それ以外の想定外
  ステータス(バックエンド未対応の`501`含む)はこのゲートウェイの
  `502 Bad Gateway`として呼び出し元へ伝える。実バイナリ2つ
  (`open-runo-router`+`open-web-server`)を実際に起動し、
  self-issueキー取得→`GET /internal/db/state/...`→open-runoの
  実`501`(in-memoryバックエンドはコミット履歴非対応)が
  `502`として正しく伝播することを実HTTPで確認済み。
  **`OPEN_RUNO_ENDPOINT`はopen-runo/poem-cosmo-tauriどちらも指せる**
  (2026-07-14確認) — `DbStateReader`はエンドポイントのURLだけで動作し、
  どちらの実装かを区別するコードを持たない。poem-cosmo-tauri側に同じ
  `GET /api/db/:table/:key/at/:commit_id`が実装された時点で(同日
  ミラー完了、詳細はpoem-cosmo-tauri側CLAUDE.md参照)、`open-web-server`
  側のコード変更なしにそちらへも接続できることを実バイナリ3つ
  (`open-runo-router`・poem-cosmo-tauri版`open-runo-router`・
  `open-web-server`)で実証済み。

## 型の共有方針

現時点では各リポジトリが独立した Cargo workspace のため、`MutationRequest` /
`MutationReceipt` は open-web-server-core / open-runo 双方で同じ JSON スキーマとして
個別定義している。将来的に `open-cosmo` 共通クレートとして切り出し、
3プロジェクトが `cargo` 依存で共有する方針（ROADMAPに記載）。

## 開発時の起動順序

```bash
# 1. aruaru-db を起動 (pgwire :5432, GraphQL :4000)
cargo run -p aruaru-server -- --data ./data --raft-id 1

# 2. open-runo を起動 (Federation Gateway)
cargo run -p open-runo-gateway

# 3. open-web-server を起動 (Client向けAPI)
OPEN_RUNO_ENDPOINT=https://127.0.0.1:8443 cargo run -p open-web-server-gateway
```

## 障害時の挙動

- open-runo または aruaru-db が一時的に応答しない場合、open-web-server は
  指数バックオフで最大5回リトライする（`LedgerConfig::max_retries`）
- リトライ上限に達した場合、クライアントには `409 Conflict` 相当で
  「未確定」を明示的に返す（サイレントな成功扱いは絶対にしない）
- WAL に書き込み済みのリクエストは、プロセス再起動後もリプレイ可能なため、
  ネットワーク瞬断があってもデータそのものは消失しない
