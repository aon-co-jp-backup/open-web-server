# aruaru-db / open-runo / open-web-server 連携ガイド

## 役割分担

| プロジェクト | 役割 | 技術スタック |
|---|---|---|
| **open-web-server** | クライアント向け入口。REST/GraphQL API、課金・決済のWAL先行書き込み | Rust + Poem |
| **open-runo** | Graph Federation Gateway。認証・Rate Limit・監査・AIルーティングの一元管理 | Rust + Poem |
| **aruaru-db** | 分散 Git-on-SQL データベース。Raftによる強整合コミット、Git的な変更履歴 | Rust |

## 連携インターフェース

- open-web-server → open-runo: `POST /internal/db/mutate` (JSON, `MutationRequest`)
- open-runo → aruaru-db: `aruaru-wire` (pgwire互換) 経由でSQL実行、または
  `aruaru-graphql` 経由でGraphQL Mutation実行
- 応答は必ず `MutationReceipt { idempotency_key, committed, db_commit_id, committed_at }`
  の形で返し、`db_commit_id` が入って初めて「確定」とみなす

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
