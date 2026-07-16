# テナント別TLS終端(Phase 1、2026-07-16)

## これは何か

`open-web-server`自体が、SNI(TLS ClientHelloのserver_name)に応じて
テナント(ドメイン)ごとに別々の証明書を選んで応答できるようにする機能。
これが無い間は、`open-web-server`は複数ドメインを1プロセスで動的に
振り分ける`tenant_router::TenantRegistry`(HTTPルーティング)を既に持って
いても、**TLS終端自体は実nginx/Apache(`open-easyweb`が生成する
vhost経由)に依存**しており、「open-web-server自体がApache+Nginx
ハイブリッド相当のWebサーバーになる」という目標に対してこの部分が
欠落していた。本フェーズはその欠落を埋める第一歩。

## 実装

- `open-web-server-wire::TenantCertResolver`
  (`crates/open-web-server-wire/src/tls.rs`): `rustls::server::
  ResolvesServerCert`実装。ホスト名(小文字正規化)→`CertifiedKey`の
  辞書。`upsert_pem(host, cert_chain_pem, key_pem)`・
  `upsert_from_files(host, cert_path, key_path)`・`remove(host)`・
  `contains(host)`。
- `open-web-server-wire::build_tenant_server_config(resolver)`:
  上記リゾルバを使う`rustls::ServerConfig`を組み立てる(クライアント
  認証は行わない——このアプリの認証はHTTP層のAPIキー/管理トークンで
  あり、mTLSは既存のバックエンド間4層防御通信の方に別途ある)。
- `open-web-server-gateway`(`crates/open-web-server-gateway/src/
  main.rs`): `OPEN_WEB_SERVER_TLS_BIND`(例: `0.0.0.0:8443`)が設定
  されている場合のみ、`accept_tls_loop`が上記`ServerConfig`で
  TLSリスナーを起動する。ルーティングロジック(`dispatch`/`route`)は
  既存のプレーンHTTPリスナーと完全に共有——違いはハンドシェイク層のみ。
  未設定時は従来通りプレーンHTTPのみで動作する(既存動作を壊さない)。
- 管理API(`crates/open-web-server-gateway/src/handlers/tls.rs`、
  既存の`OPEN_WEB_SERVER_ADMIN_TOKEN`認証を再利用):
  - `POST /admin/tenants/:host/tls` `{"cert_pem": "...", "key_pem": "..."}`
    — 証明書チェーン+秘密鍵を登録・更新(冪等、証明書ローテーションにも
    使う)。
  - `DELETE /admin/tenants/:host/tls` — 登録済み証明書を削除(未登録でも
    冪等に成功)。
  - 証明書登録(`/tls`)とHTTPルーティング登録(`/admin/tenants`本体)は
    意図的に独立した操作——TLS終端だけ先に有効化してからルーティングを
    追加する運用(またはその逆)を妨げない。

## 検証(実TLSハンドシェイク、新規テストテナントのみ・本番nginx変更無し)

1. `cargo test -p open-web-server-wire tls::` — `TenantCertResolver`が
   2つの異なるSNI名に対して実際に異なる証明書を返すことを、本物の
   TLS 1.3ハンドシェイク(実TCPループバック、`rcgen`の使い捨て自己署名
   証明書2組)で証明(`real_tls_handshake_resolves_different_cert_per_sni`)。
2. `cargo test -p open-web-server-gateway
   tests::tls_admin_registration_enables_real_tls_handshake_and_dispatch`
   — 証明書登録 → `accept_tls_loop`が実際にそのSNI名向けTLS
   ハンドシェイクに成功 → TLS越しの`GET /healthz`が実際に`dispatch()`
   まで届き200を返す、というエンドツーエンドの経路を実TCP上で証明。
3. どちらのテストも`127.0.0.1`のエフェメラルポート上で完結し、
   `aruaru.tokyo`/`audiocafe.tokyo`の実nginx設定・実証明書には一切
   触れていない。

## 手動での動作確認(ローカル、新規テストテナント推奨)

```bash
# 1. 自己署名証明書を用意(例: rcgen不要、opensslでも可)
openssl req -x509 -newkey rsa:2048 -nodes -days 30 \
  -subj "/CN=tls-smoketest.local" \
  -keyout /tmp/tls-smoketest-key.pem -out /tmp/tls-smoketest-cert.pem

# 2. TLSリスナーを有効にして起動(通常のOPEN_WEB_SERVER_BINDと併用)
OPEN_WEB_SERVER_BIND=127.0.0.1:8080 \
OPEN_WEB_SERVER_TLS_BIND=127.0.0.1:8443 \
cargo run -p open-web-server-gateway

# 3. 証明書を登録
curl -X POST http://127.0.0.1:8080/admin/tenants/tls-smoketest.local/tls \
  -d "{\"cert_pem\": \"$(cat /tmp/tls-smoketest-cert.pem | sed 's/$/\\n/' | tr -d '\n')\", \
       \"key_pem\": \"$(cat /tmp/tls-smoketest-key.pem | sed 's/$/\\n/' | tr -d '\n')\"}"

# 4. TLS越しにhealthzを叩く(自己署名なので-kが必要)
curl -k --resolve tls-smoketest.local:8443:127.0.0.1 https://tls-smoketest.local:8443/healthz
```

## 意図的にスコープ外とした点(次回フェーズ候補)

- **ACME自動取得**: `poem-cosmo-tauri`側に既に実装・テスト済みの手書き
  ACMEクライアント(HTTP-01/DNS-01/TLS-ALPN-01、`crates/
  open-runo-router/src/acme.rs`)があり、本フェーズと同じ設計判断
  (外部フレームワーク非依存の自前実装)に沿っている。次回フェーズで
  この`ChallengeStore`+3チャレンジ型実装をこちらへ移植し、取得した
  証明書を`TenantCertResolver::upsert_pem`へ自動投入する配線を追加する
  想定。**調査結果(2026-07-16、EN/JP両言語)**: 本番運用のACME
  クライアントとしては`instant-acme`(アクティブにメンテナンスされた
  pure-Rust実装、レート制限・アカウントキャッシュ等の実務上の懸念に
  対応済み)が2026年時点で推奨される選択肢である一方、既存の手書き実装は
  既にテスト済みで新規依存を追加しない——本フェーズでは既存資産の移植を
  優先し、`instant-acme`への切替は「本番運用のレート制限/アカウント
  キャッシュが実際に問題になった場合の改善候補」として明記するに留める。
- **HTTP/2・WebSocketアップグレードのTLS越し対応**: 現状`accept_tls_loop`
  は`http1::Builder`のみ(既存プレーンHTTPリスナーと同じ制約を踏襲)。
- **`tenant_router::TenantConfig`とTLS証明書登録の統合**: 現状は
  `POST /admin/tenants`(HTTPルーティング)と`POST /admin/tenants/:host/tls`
  (証明書)が別々のAPI。将来的に`TenantConfig`へ証明書パス/自動ACME
  フラグを持たせ、1回のテナント追加でHTTP+TLS+証明書取得まで一気通貫に
  できるようにする余地がある。
