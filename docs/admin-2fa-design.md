# 管理API 2段階認証 設計書

**状態: 設計のみ(実装・コミット・本番変更なし)** / 作成日: 2026-10-28

## Summary (English)

The admin API of `open-web-server` (the edge server holding ports 80/443 on the production VPS) is protected by one static shared token (`OPEN_WEB_SERVER_ADMIN_TOKEN`, header `x-admin-token`). A mail-OTP + TOTP login flow already exists (`handlers/admin_login.rs`, Cargo feature `admin-2fa`, off by default), but it does **not** protect anything today: `check_admin_auth` falls back to the static token, so 2FA is bypassable as long as the static token works. Also, the static token is also used by a machine caller (the certbot deploy hook), so 2FA cannot simply be forced everywhere.

Recommendation (details below):
1. Split callers into **humans** (2FA login → short-lived Bearer key) and **machines** (a new narrowly scoped token that can only call the TLS-registration endpoint, accepted only from loopback).
2. Demote the static admin token to a **break-glass** credential accepted **only from loopback** (usable via SSH / ConoHa console), which doubles as the lockout-recovery path.
3. Rotate the (leaked) static token first, audit what the leaked token could have created (Bearer keys, 2FA enrolments, tenants, TLS), then roll out in phases with a rollback at each step.
4. Build the `admin-2fa` binary in CI (not on the VPS: ~200 MB free RAM, swap nearly full), against a glibc old enough for the VPS.
5. Code review of `admin_login.rs`: the unauthenticated-TOTP-setup flaw found in `open-english` is **not** present (enroll/confirm require admin auth), but there are six hardening gaps (no rate limit on OTP request/verify, OTP-mail bombing/invalidation, TOTP file permissions, fail-open when the token env var is unset, enroll can overwrite a confirmed enrolment, TOTP code replay).

---

## 1. 現状(コードで確認した事実)

| 項目 | 内容 | 根拠 |
|---|---|---|
| 認証の入口 | `check_admin_auth` が (a) 送信元IP許可リスト(`OPEN_WEB_SERVER_ADMIN_ALLOWED_IPS`、設定時のみ)、(b) `Authorization: Bearer`(KeyGuardian発行、`roles` 空または `admin` ならフルアクセス)、(c) 上記で通らなければ静的トークン `x-admin-token` にフォールバック、の順 | `handlers/tenants.rs` `check_admin_auth` |
| **静的トークン未設定時** | `OPEN_WEB_SERVER_ADMIN_TOKEN` が無いと `check_static_admin_auth` は **無条件で通す(fail-open)** | 同 `check_static_admin_auth` |
| 2段階ログイン | `POST /admin/login/request-otp`(認証不要・メールOTP送信)→ `POST /admin/login/verify`(メールOTP+TOTP 両方正しければ 12時間有効の Bearer キー発行、`roles=["admin"]`) | `handlers/admin_login.rs` |
| 2FA登録 | `POST /admin/2fa/enroll`・`/confirm` は `check_admin_auth` 必須 | 同 |
| feature | `admin-2fa` は既定オフ。ルートは `#[cfg(feature="admin-2fa")]` のため、**feature無しバイナリでは `/admin/login/*` 自体が存在しない** | `Cargo.toml`、`lib.rs` 225-233行 |
| 機械の呼び出し元 | certbot deploy hook が `http://127.0.0.1/admin/tenants/$name/tls` を静的トークンで呼ぶ。トークンは `systemctl show` の Environment / EnvironmentFiles から実行時に読む | `scripts/certbot-deploy-hook-open-web-server.sh` |
| 公式リリースのビルド | `.github/workflows/release.yml` は `cargo build --release --bin open-web-server`(**feature無し**)を `ubuntu-latest` で実行 | 同ファイル 38行 |

**未確認(推測せず確認が必要)**
- 本番VPSの実行ファイルが `admin-2fa` 有効かどうか(確認方法: §5 手順0)。
- 本番バイナリのfeature一式(過去の記録では `fastcgi-client,acme,ddns,sftp,upnp,domain-watchdog` 等。再ビルド時に1つでも落とすと機能退行する)。
- 静的トークンを使う他の呼び出し元(`open-easy-web` の登録処理、各サイトの自動化、運用スクリプト等)。**棚卸しが完了するまで段階2以降に進めないこと。**
- 本番OSのバージョンと glibc バージョン(AlmaLinux 8/9/10 のどれか)。
- VPSで `OPEN_WEB_SERVER_SMTP_*`、`OPEN_WEB_SERVER_2FA_STORE_PATH`、`OPEN_WEB_SERVER_KEY_STORE_PATH` が設定済みか。
- 自動アップデート(`auto-update` feature)が本番で有効か。有効だと、GitHub Releases の feature無しバイナリで `admin-2fa` 付きの自前ビルドが**上書きされる**危険がある(§5)。

---

## 2. 重要な設計上の前提: いまの2FAは「飾り」になっている

`check_admin_auth` は Bearer が無効なら静的トークンに戻る。つまり **静的トークンが通る限り、2段階認証を経由しなくても全管理操作ができる**。さらに `enroll` は静的トークンで呼べ、既存ownerのTOTP秘密鍵を上書きできる。したがって「`admin-2fa` を有効にして再ビルドするだけ」では何も守れない。**静的トークンを人間経路から外すこと**が設計の核心になる。

---

## 3. 設計: 経路の分離

### 3.1 三種類の資格情報

| 種別 | 用途 | 認証方法 | 到達できる範囲 | 到達元 |
|---|---|---|---|---|
| **人用** | 管理者が行うテナント追加・vhost変更・キー管理など全て | メールOTP + TOTP → Bearer キー(有効期限は現行12時間。推奨は**2時間**に短縮) | `/admin/*` 全体 | 公開側から可(IP許可リストで絞れれば尚良い) |
| **機械用(新設)** | certbot hook の TLS登録のみ | 専用トークン `OPEN_WEB_SERVER_TLS_TOKEN`(ヘッダは `x-admin-token` とは別名の `x-tls-token` を推奨) | `POST/DELETE /admin/tenants/:host/tls` **のみ** | **ループバックのみ**(送信元が 127.0.0.1/::1 以外なら拒否) |
| **非常用(break-glass)** | ロックアウト時の復旧、初回2FA登録 | 既存の静的トークン `OPEN_WEB_SERVER_ADMIN_TOKEN`(ローテーション後) | `/admin/*` 全体 | **ループバックのみ**(SSH / ConoHaコンソール経由) |

### 3.2 機械用の選択肢比較と推奨

| 案 | 内容 | 長所 | 短所 |
|---|---|---|---|
| A. 範囲限定トークン + ループバック限定(**推奨**) | 新しい `x-tls-token` は TLS登録の2エンドポイントだけ通す。さらに送信元がループバックでなければ拒否 | 漏れても被害はTLS証明書の差し替えまでに限定。人間が使う公開経路から完全に切り離せる。hookの変更が小さい(ヘッダ名とトークン名のみ) | 小規模なコード変更が必要(`check_admin_auth` に経路別判定を追加) |
| B. 静的トークン + ループバック/IP許可リストのみ(コード変更なし) | 既存の `OPEN_WEB_SERVER_ADMIN_ALLOWED_IPS=127.0.0.1` を使う | 実装不要 | 許可リストは**全 `/admin/*` 一律**に効く。人間も公開側から入れなくなり、人用経路(2FA)が成立しない。hook用トークンが全権限のまま |
| C. hook が管理APIを呼ばない(ファイル配置 + シグナル/ファイル監視で再読込) | 証明書を `TLS_CERT_DIR` に置き、プロセスに再読込させる | トークン自体が不要になる | 再読込機構が現状ない(新機能)。大きい変更 |

**推奨は A。** 理由: 変更が小さく、漏えい時の被害が最小で、人用経路と完全に分離できる。B は人用経路を壊す。C は将来の最終形としては良いが今回の目的に対して過大。

> `ip_allowlist` は `PeerAddr`(TCP接続元)を見るため、open-web-server が入口であり前段のリバースプロキシが無い現構成ではヘッダ偽装の心配がない(前提: 前段にnginx等を戻した場合は成立しないので、その場合は設計を見直すこと)。

### 3.3 必要なコード変更の概要(実装は別作業)
1. `check_admin_auth` を**経路別**にする: (i) TLS登録2エンドポイントでは `x-tls-token`(定数時間比較)+ループバックを許可、(ii) 静的トークンは**ループバックのみ**、(iii) 人用は Bearer(2FA経由)のみ。
2. `check_static_admin_auth` を **fail-closed** にする(トークン未設定なら拒否、`503`)。開発用に残すなら `OPEN_WEB_SERVER_ADMIN_INSECURE_DEV=true` のような明示オプトインにする。
3. §6 のハードニング(レート制限など)。
4. 切替をスイッチ化する: 環境変数 `OPEN_WEB_SERVER_ADMIN_STATIC_TOKEN_MODE=any|loopback|off`(`any`=現行互換)。これにより段階導入と**再ビルド無しのロールバック**ができる。
5. リリースワークフローに `admin-2fa` を含む本番用ビルド(§5)。

---

## 4. 静的トークンのローテーション(現行は漏えい扱い)

**重要**: 漏えいしたトークンで既に何かが作られている可能性があるため、ローテーションだけでは足りない。**交換と監査をセットで行う。**

### 4.1 事前確認(変更前)
- 現在の `domains.toml` の件数、`tls-certs/` のファイル数、`web_vhosts.toml`・`redirects.toml` の有無をメモ(再起動で消えないことの確認用。過去に再起動でTLS証明書ディレクトリが空だったことがある)。
- `domains.toml`、`web_vhosts.toml`、`redirects.toml`、キーストア、2FAストア、`tls-certs/` のバックアップを取る(コピー先は暗号化された場所)。
- トークンが書かれている場所の洗い出し: systemd の Environment/EnvironmentFile、`open-easy-web` 等の設定、運用スクリプト、シェル履歴、**過去の作業ログ・チャット出力**(漏えいの原因なので、ログ側にも残っていないか確認)。

### 4.2 手順
1. 新トークンを生成(例: `openssl rand -hex 32`)。画面に表示せずファイルへ直接書く(権限 600、所有 root)。
2. systemd の `EnvironmentFile`(または drop-in)の `OPEN_WEB_SERVER_ADMIN_TOKEN` を新値に差し替える。drop-in を変更したら `systemctl daemon-reload`。
3. 他の呼び出し元(棚卸しで見つけたもの)を新値に更新。certbot hook は `systemctl show` から実行時に読むので、トークン名を変えなければ自動追従する。
4. `systemctl restart open-web-server`(**再起動が必要**: トークンは起動時の環境から読まれる)。直後に §4.3 の確認。
5. 旧トークンが401になることを確認。

### 4.3 漏えい期間中に作られた可能性のあるものの監査
- `GET /admin/tenants`(未知のホストがないか)、`domains.toml` を直近のバックアップ/git履歴と差分比較。
- `GET /admin/keys`(件数のみ返る)で想定外のキーが無いか。**不審な場合は `POST /admin/keys/revoke` で owner ごとに失効**(キーはownerごとにしか失効できない。想定外のownerが見えない場合は、キーストアファイルを退避して空から始めるのが確実。ただし全員再ログインになる)。
- 2FAストア(`OPEN_WEB_SERVER_2FA_STORE_PATH`)の owner 一覧を確認。想定外の owner/メールアドレスがあれば削除(APIに削除手段がないため、ファイルを直接編集 → 再起動)。
- TLS: 登録済み証明書の発行元・期限を確認(`openssl s_client` で各ホストを確認)。
- 同じトークンを他の環境(`open-english` 等)で使い回していないか。

---

## 5. `admin-2fa` 有効化ビルドと VPS のメモリ制約

VPSは空きメモリ約200MB、スワップほぼ満杯のため、**VPS上で `cargo build --release` を行わない**(OOM killer が本番プロセスを巻き込むおそれ)。

### 手順0: 現状確認(VPSで読み取りのみ)
```
strings /root/open-web-server/target/release/open-web-server | grep -c 'admin/login/request-otp'
```
0 なら現バイナリは `admin-2fa` 無効。ついでに `--version` と、稼働バイナリの更新日時も控える。

### CIでのビルド(推奨)
- GitHub Actions で本番用ワークフローを新設(既存 `release.yml` は feature無しのままなので流用しない)。
  - features は**現本番と同一 + `admin-2fa`**。現本番の一覧は手順0とは別に、デプロイ時のコマンド履歴/ドキュメントで確認し、**確定してから**ビルドする(未確認)。
  - **glibc互換**: `ubuntu-latest` は glibc が新しく、AlmaLinux 8/9 では `GLIBC_2.xx not found` で起動しない可能性が高い。対策は (1) `almalinux:<本番と同じ版>` コンテナ内でビルドする、または (2) `cargo zigbuild --target x86_64-unknown-linux-gnu.2.28` 等で glibc 下限を指定する。本番のglibc版は `ldd --version` で確認(未確認)。
  - 成果物は SHA-256 を添えて Artifact/Release(非公開または下書き)に置く。
- VPSへは `scp` で `open-web-server.new` として置き、`sha256sum` で照合 → 起動前検証(`./open-web-server.new --version`)→ 差し替え時に旧バイナリを `open-web-server.bak-<日付>` として残す。
- **自動アップデートに注意**: `auto-update` が有効だと、公式リリース(feature無し)で自前バイナリが上書きされ、`/admin/login/*` が消える。段階1の前に `GET /admin/auto-update` で状態を確認し、必要なら無効化するか、リリースの成果物自体を `admin-2fa` 入りに揃える。
- どうしてもVPS上でビルドする場合の最終手段: 別マシンで作った `target` ディレクトリは持ち込まず、スワップ増設 + `cargo build -j1` + 本番プロセスのメモリ上限確認が前提。OOM リスクがあるため非推奨。

### 必要な環境変数(機密値は文書に書かない)
`OPEN_WEB_SERVER_SMTP_HOST/PORT/USERNAME/PASSWORD/FROM`(メールOTP送信)、`OPEN_WEB_SERVER_2FA_STORE_PATH`(未設定だと再起動のたびにTOTP再登録)、`OPEN_WEB_SERVER_KEY_STORE_PATH`(未設定だと再起動のたびにログインキー消失)、`OPEN_WEB_SERVER_ADMIN_TOKEN`、(新設)`OPEN_WEB_SERVER_TLS_TOKEN`、(新設)`OPEN_WEB_SERVER_ADMIN_STATIC_TOKEN_MODE`。

---

## 6. `admin_login.rs` / `two_factor.rs` のコードレビュー

### 6.1 `open-english` の脆弱性と同種の問題はあるか
**ない(確認済み)。** `open-english` では TOTP設定APIが無認証で秘密鍵を返していた。本リポジトリでは `enroll` と `confirm` の両方が先頭で `check_admin_auth` を呼ぶ。`request_otp` / `verify_login` / `verify_suspension_override` は認証なしだが、秘密鍵や機密を返さない(`request_otp` は常に同じ200を返し、owner列挙もできない)。

ただし**同種の「設計上の弱点」は別の形である**:
- **(重大) 静的トークンで enroll が通り、既存ownerの秘密鍵を上書きできる**(`TwoFactorStore::enroll` は既存の確認済み登録を無条件に置き換える)。静的トークンを持つ者は 2FA を迂回して自分のTOTP/メールを登録し、ログインキーを発行できる。→ §3 の「静的トークンはループバック限定」で緩和。さらに、確認済みownerへの `enroll` は拒否(再登録は明示的な `reset` を break-glass 限定で)にする。
- **(重大) 静的トークン未設定時は fail-open**。環境変数が欠落すると `enroll` を含む全管理APIが無認証になる。→ fail-closed 化。

### 6.2 その他の指摘(要ハードニング)
| # | 指摘 | 影響 | 対策案 |
|---|---|---|---|
| 1 | `request_otp` にレート制限なし。呼ぶたびに `issue_email_otp` が**既存チャレンジを上書き** | 認証なしで (a) 管理者宛のメール爆撃、(b) 正規の管理者が受け取ったOTPを無効化し続けるログイン妨害 | owner単位+IP単位のレート制限(例: 5分に3回)。有効なチャレンジがある間は再発行しない(または再送上限) |
| 2 | `verify_login` に試行回数制限なし(コメントでも限界として自認) | メールOTP(6桁)+TOTP(6桁)の同時一致が必要で確率は極めて低いが、無制限に試せる | owner単位で連続失敗5回 → 15分ロック。失敗時はチャレンジを無効化する |
| 3 | TOTPシークレットの保存ファイルがumask依存(`std::fs::write` のまま、権限指定なし) | 同一ホストの他ユーザーが読める可能性。読めれば第二要素が無効 | 600で作成し、ディレクトリも700。既存ファイルは `chmod 600` を段階0で確認 |
| 4 | TOTPコードの使い回しが可能(最終使用ステップを記録していない) | 90秒窓内の盗み見コードの再利用 | owner毎に最後に受理したステップ番号を保存し、それ以下を拒否 |
| 5 | `confirm` が成功した後も再 `confirm` 可、OTPメールの送信先 `email` は enroll 時に任意指定 | 静的トークン保持者(=現状は全権)に限られる。§6.1で緩和 | `enroll` で確認済みownerは拒否 |
| 6 | `verify_suspension_override`(`/admin/2fa/verify`)は認証なしでTOTP単体の試行ができ、試行制限なし | 成功しても「既に有効なキーを持つownerの一時停止解除」にしかならない(それ自体で管理者にはなれない) | #2 と同じ制限を適用 |
| 7 | ログインキーのTTLが12時間。メールOTPはメモリ上のみ(再起動で消える) | 窃取時の窓が長い | 2時間へ短縮。メモリ上のOTPは問題なし |
| 8 | ログに機密が出ていないか | 未確認 | `tracing` 出力と `enroll` レスポンス(`secret_base32` と `qr_svg` を含む)をリバースプロキシ/アクセスログが記録していないか確認(`OPEN_WEB_SERVER_ACCESS_LOG_PATH` はボディを記録しない設計だが、実機では未確認) |

---

## 7. ロックアウト時の復旧手順

人用経路(2FA)は、メール不達・スマホ紛失・SMTP障害・ストア破損で使えなくなりうる。復旧は**サーバーのコンソールに入れること**を前提にする(SSH、または ConoHa のコントロールパネルのコンソール)。

1. 非常用(break-glass): 静的トークンはループバックでのみ有効。サーバーに入り `curl http://127.0.0.1/admin/...`(ヘッダは `x-admin-token`)で操作する。
2. 2FAの再登録: ループバックで `POST /admin/2fa/enroll`(新設の reset 経路)→ QR(`qr_svg`)は**ローカルPCに保存して表示**する(例: `ssh vps 'curl ...' | jq -r .qr_svg > qr.svg` をローカルで実行し、手元で開く)。サーバー側のシェル履歴や画面に秘密鍵を残さない。完了後に `confirm`。
3. 最悪の場合: `ADMIN_STATIC_TOKEN_MODE=any` に戻して再起動(再ビルド不要)、または §5 で残した旧バイナリへ戻す。
4. 事前準備として、**今のうちに**ConoHaコンソールでログインできること(rootパスワード/SSH鍵の所在)を確認しておく(未確認)。復旧手段そのものが使えないと、二重のロックアウトになる。
5. 復旧用に 2 人目の管理者(別owner・別メール・別スマホ)を登録しておくと、1人の端末紛失で詰まない(推奨)。

---

## 8. 段階導入・実機確認・ロールバック

各段階は**次に進む前に必ず実機確認**し、問題があれば直前の状態へ戻す。再起動の前後で `tls-certs/` のファイル数・`domains.toml` の件数・全ドメインの HTTPS 疎通を確認する。

| 段階 | 内容 | 実機確認 | ロールバック |
|---|---|---|---|
| 0 | 事前確認: §1の未確認事項、バックアップ、呼び出し元棚卸し、ConoHaコンソールの確認、`auto-update` 状態確認、TOTPファイル権限確認 | 手順0(`strings`)、`ldd --version`、`GET /admin/auto-update` | 変更なし |
| 1 | 静的トークンのローテーション + 監査(§4)。コード変更なし | 旧トークン 401、新トークン 200、certbot hook の `--dry-run`(`certbot renew --dry-run`)が成功、全ドメインHTTPS | 旧値を `EnvironmentFile` に戻して再起動(ただし旧値は漏えい扱いなので短時間のみ) |
| 2 | 新バイナリ(`admin-2fa` + 経路分離 + fail-closed + レート制限)をCIでビルドし、**ステージング(別ポート/別環境)**で単体検証。本番へはまだ入れない | テストスイート、ループバック限定・範囲限定トークンの拒否/許可の自動テスト、OTP送信の実メール到達確認 | 本番に触れていないので不要 |
| 3 | 本番へバイナリ差し替え。`STATIC_TOKEN_MODE=any`(現行互換)のまま起動し、新エンドポイントの存在だけ確認。TLS用の新トークンを設定 | `/admin/login/request-otp` が200、旧挙動(静的トークン)が引き続き通る、全ドメインHTTPS、`tls-certs/` 件数不変 | 旧バイナリ `.bak-<日付>` に戻して再起動 |
| 4 | certbot hook を新しい範囲限定トークンへ切替(`x-tls-token`)。`renewal-hooks/deploy` を更新 | `certbot renew --force-renewal --cert-name <テスト用1ドメイン>` で実際に反映されること、`POST /admin/tenants/.../tls` が非ループバックから401/403 | hook を旧版に戻す(静的トークンはまだ有効) |
| 5 | 2FA登録: 管理者(+予備管理者)を enroll → confirm、実際に `request-otp` → `verify` でログインして Bearer キーで `GET /admin/tenants` が通ることを確認 | メール到達、スマホのTOTP一致、間違いコードで401、レート制限の発動 | 登録を削除(ファイル編集 + 再起動) |
| 6 | `STATIC_TOKEN_MODE=loopback` に切替して再起動 | 公開側から静的トークンで 401/403、ループバックでは 200、2FAログインは引き続き可、certbot hook は成功、全ドメインHTTPS | `STATIC_TOKEN_MODE=any` に戻して再起動(再ビルド不要) |
| 7 | 数日〜1か月運用して問題がなければ `STATIC_TOKEN_MODE=off`(break-glassも停止)を検討。ただし復旧手段の喪失を意味するため**推奨しない**(ループバック限定で常に残す) | — | `loopback` に戻す |

**共通の再起動チェック**: 再起動後すぐに (1) `ls tls-certs | wc -l` が再起動前と一致、(2) `GET /admin/tenants`(約90件)の件数一致、(3) 代表ドメインへの HTTPS 200、(4) systemd ログに起動エラーなし、を確認する。再起動中の瞬断は避けられない(数秒)ので、時間帯を選ぶ。

---

## 9. ユーザー確認が必要な未決事項

- [ ] 管理者は何人か(メールアドレスと、予備管理者の有無)。メールOTPの送信元SMTP(サービス・アカウント)はどれを使うか。
- [ ] 静的トークンを使っている他の呼び出し元を全て教えてほしい(`open-easy-web`、各サイト、スクリプト等)。特に `open-easy-web` から `open-web-server` へテナント登録する処理は、人の操作が介在するか、自動化か。自動化なら機械用の別資格情報(登録専用トークン等)が追加で必要。
- [ ] 本番OSのバージョン(AlmaLinux 8/9/10)と、本番バイナリを現在どう作ったか(feature一式)。
- [ ] 自動アップデートを本番で使っているか(使っていると上書きの危険)。
- [ ] 機械用の推奨案A(範囲限定トークン+ループバック限定)でよいか。
- [ ] 静的トークンの最終形: ループバック限定のまま常設(推奨)か、将来的に廃止か。
- [ ] ログインキーの有効期限: 2時間(推奨)でよいか。
- [ ] 2FAを必須にする対象の範囲: 管理API全体か、特に危険な操作(キー発行・テナント/vhost変更・TLS・自動アップデート)だけか。
- [ ] ステージング環境(別VPSまたは別ポート)を用意できるか。できない場合、段階2の検証をどう代替するか。
- [ ] §4.3 の監査で不審なものが見つかった場合の対応方針(全ログインキー失効してよいか)。
- [ ] 漏えいしたトークンが載っていた出力の保管場所(チャット履歴・ログ)の扱い。
