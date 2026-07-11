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

## 冗長化された伝送経路: TCP-IP + UDP-IP (`open-web-server-wire::udp_channel`, 2026-07-11)

`CLAUDE.md` の拡張要件 (3) 「TCP-IP・UDP-IPの三層三重通信」に対する**最初の
具体的な実装**。上記「3層防御通信」はTLS/相互認証/ペイロード暗号化という
**セキュリティレイヤー**のスタックであり、単一のTCPコネクション上に積む
ものだった。今回追加した `udp_channel` はこれと直交する、別の性質の冗長化
──**伝送経路そのものの複線化**である。

```text
open-web-server-ledger::Ledger::commit()
  │
  ├─ ① WAL 先行書き込み (fsync相当)
  │
  ├─ ② TCP経由 open-runo forward  ─────────► 権威パス (db_commit_id を発行)
  │     (3層防御通信、既存)                     失敗時はリトライ、最終的に失敗なら
  │                                            commit 自体が失敗として返る
  │
  └─ ③ UDP経由 即時通知 (新規、副系)  ─────────► ベストエフォート
        tokio::spawn の fire-and-forget。
        PayloadCipher (ChaCha20-Poly1305) で暗号化 +
        HMAC-SHA256 でデータグラム単位の完全性・認証を付与。
        失敗・タイムアウトしても②をブロック・失敗させない。
```

### スコープと限界 (正直な記載)

- **再送は実装していない**。UDPは「送りっぱなしの即時通知」として扱う。
  本体データの確定 (`db_commit_id` の発行) は今まで通りTCP経由の3ホップ
  コミットのみが担う。目標アーキテクチャの「主系TCP+副系TCP+UDP」の
  三重化のうち、UDP1系のみの第一実装であり、副系TCPは未実装。
- **冪等キーによるデデュープ**: 同一 `IdempotencyKey` のミューテーションが
  TCP経由・UDP経由の両方で届いても実害がない設計 (既存の冪等性設計をUDPの
  重複・順序入れ替わりにもそのまま適用)。`udp_channel::Deduplicator` が
  受信側の集合管理を担う。
- **暗号化・認証**: UDPにはTLSが無いため、AEAD暗号化 (`PayloadCipher`) を
  機密性の主体とし、HMAC-SHA256を完全性・認証に用いる (鍵は
  `auth::MutualAuthConfig` と同じ長期共有シークレットからHKDFで導出する
  運用を想定)。
- **受信側の実配置は未接続**: 実際にどのプロセス (open-runo側) がUDP
  ソケットをlistenし、`aruaru-db`側WALと結合するかは別スコープ。本リポジトリ
  内では `open-web-server-ledger::Ledger` が UDP送信側のみを結線しており、
  `enable_udp_redundant_path()` で任意に有効化できる (未呼び出しなら従来通り
  TCPのみで動作)。
- **検証**: `open-web-server-wire::udp_channel` の単体テストに加え、実
  `tokio::net::UdpSocket` (127.0.0.1 loopback) を使った結合テストで
  (a) 暗号化・HMAC検証・デデュープの一連の流れ、(b) 改ざんデータグラムの
  拒否、を実証。`open-web-server-ledger` 側では実TCPソケットのモック
  open-runoサーバを使い、(c) UDP経由通知がTCP確定と並行して正しく届き
  デデュープされること、(d) UDP宛先が完全に到達不能でもTCP経由の権威
  パスは影響を受けずコミットが成功すること、を実証済み。
