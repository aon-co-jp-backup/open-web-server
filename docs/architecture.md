# open-web-server アーキテクチャ

## 全体構成

```text
┌────────────────────────────────────────────────────────────────┐
│ Client (3D Game Client / 決済端末 / Webブラウザ)                    │
└───────────────────────────┬────────────────────────────────────┘
                            │  3層防御通信 (open-web-server-wire)
                            ▼
┌────────────────────────────────────────────────────────────────┐
│ open-web-server  (Rust + Poem)                                  │
│  - REST / GraphQL Gateway                                       │
│  - Idempotency-Key 必須ミドルウェア                                │
│  - ローカル WAL 先行書き込み (open-web-server-ledger)               │
└───────────────────────────┬────────────────────────────────────┘
                            │  3層防御通信
                            ▼
┌────────────────────────────────────────────────────────────────┐
│ open-runo  (Rust + Poem, Graph Federation Gateway)               │
│  - 認証・Rate Limit・監査ログの一元管理                             │
│  - VersionlessAPI / AI ルーティング                                │
└───────────────────────────┬────────────────────────────────────┘
                            │  3層防御通信
                            ▼
┌────────────────────────────────────────────────────────────────┐
│ aruaru-db  (Rust, 分散 Git-on-SQL データベース)                     │
│  - openraft による分散強整合コミット                                │
│  - コミットごとに commit_id (Git的ハッシュ) を発行、監査・巻き戻し可能   │
└────────────────────────────────────────────────────────────────┘
```

## 3層防御通信 (open-web-server-wire)

すべてのサービス間通信 (Client→open-web-server, open-web-server→open-runo,
open-runo→aruaru-db) は同一方針の3層で保護する。

1. **第1層 (tls.rs)**: TLS 1.3 (rustls) による伝送路暗号化
2. **第2層 (auth.rs)**: 相互認証。HKDF によるチャレンジ&レスポンスで、
   「正しいサービス同士の通信であること」を毎回検証する
3. **第3層 (payload_crypto.rs)**: ChaCha20-Poly1305 (AEAD) によるアプリケーション層
   ペイロード暗号化。TLS がロードバランサ等で終端されても、業務データ自体は
   暗号化されたまま流れる

## 課金/金融データの消失防止 (open-web-server-ledger)

1. クライアントが `Idempotency-Key` を発行してリクエスト
2. open-web-server がローカル WAL に先行書き込み（プロセス再起動時にリプレイ可能）
3. open-runo 経由で aruaru-db に転送し、Raft 分散合意でコミット
4. aruaru-db が発行した `commit_id` を受け取るまで、クライアントには「確定」を返さない
5. 同一 `Idempotency-Key` の再送は、常に同じ結果を返す（二重課金・二重付与の防止）

これにより、途中経路の瞬断やリトライが発生しても、書き込みは
**exactly-once相当（at-least-once送信 + 冪等キーによる重複排除）** として扱われる。

## 可観測性 (`open-web-server-gateway::telemetry`)

`grant_item`/`charge` ハンドラは `#[tracing::instrument]` でスパン化されており、
`tracing-opentelemetry` レイヤー経由で OpenTelemetry の Tracer に橋渡しされる。

- `OTEL_EXPORTER_OTLP_ENDPOINT` が設定されていれば OTLP/HTTP (protobuf) で
  その Collector へバッチエクスポートする。
- 未設定時は `opentelemetry-stdout` で標準出力にスパンを書き出す
  (Collector 未起動のローカル開発環境向けフォールバック)。
- `main()` の末尾で `TelemetryGuard::shutdown()` を呼び、プロセス終了前に
  バッファ済みスパンを確実にフラッシュする。

現時点では `open-web-server-gateway` 単体でのスパン生成のみ。
`open-runo`/`aruaru-db` 側が同じ Trace Context を伝播・エクスポートするように
なれば、`Client → open-web-server → open-runo → aruaru-db` 全体を1本の
分散トレースとして追跡できるようになる(両リポジトリの対応状況は未確認)。
