//! テナント(ドメイン)管理用の内部API。
//!
//! ドメイン追加/削除がプロセス再起動やポート個別割り当てを伴わないことを
//! 実際のハンドラとして示す(`tenant_router::TenantRegistry` 参照)。
//! 本番運用では認証・監査ログを追加すべきだが、第一実装としてはレジストリ
//! 操作の配線を優先する。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};

use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;
use crate::tenant_router::{TenantConfig, TenantError};

const ADMIN_TOKEN_HEADER: &str = "x-admin-token";

/// 定数時間文字列比較(2026-07-30追記、`aruaru-server`側で発見・修正した
/// タイミングサイドチャネル〈CWE-208〉対策と同じ理由・同じ実装。素の
/// `==`比較は一致した先頭バイト数に応じてわずかに応答時間が変化しうる
/// ため、外部から到達可能な管理トークン比較にこの種の脆弱性を持ち込ま
/// ないよう、全バイトを走査してから結果をXOR累積で判定する。新規crate
/// 依存は追加せず、この用途限定の最小実装とした。
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= x ^ y;
    }
    diff == 0
}

/// 静的トークン(`x-admin-token`)の受け入れ範囲
/// (`OPEN_WEB_SERVER_ADMIN_STATIC_TOKEN_MODE`、2026-10-28新設)。
/// 既定は`Any`(従来互換)。再ビルドなしで`loopback`/`off`へ段階的に
/// 絞り、また`any`へ戻せる(`docs/admin-2fa-design.md` §3.3・§8)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StaticTokenMode {
    Any,
    Loopback,
    Off,
}

impl StaticTokenMode {
    fn parse(raw: Option<&str>) -> Self {
        match raw.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            None | Some("") | Some("any") => Self::Any,
            Some("loopback") => Self::Loopback,
            Some("off") => Self::Off,
            Some(other) => {
                // 綴り間違いで全開放になるのを避けるため、不明な値は
                // 安全側(loopback)に倒して警告する。
                tracing::warn!(value = other, "unknown OPEN_WEB_SERVER_ADMIN_STATIC_TOKEN_MODE, treating as 'loopback'");
                Self::Loopback
            }
        }
    }

    fn from_env() -> Self {
        Self::parse(std::env::var("OPEN_WEB_SERVER_ADMIN_STATIC_TOKEN_MODE").ok().as_deref())
    }
}

/// 送信元がループバック(127.0.0.0/8, ::1, IPv4射影の::ffff:127.x)か。
/// `ip_allowlist`と同様、前段にリバースプロキシが無い(open-web-serverが
/// 入口)構成が前提で、`X-Forwarded-For`等のヘッダは一切見ない。
pub(crate) fn is_loopback_ip(ip: std::net::IpAddr) -> bool {
    ip.to_canonical().is_loopback()
}

/// 接続元が不明(`PeerAddr`無し)の場合は安全側で`false`。
fn peer_is_loopback(req: &Request<Incoming>) -> bool {
    req.extensions()
        .get::<crate::PeerAddr>()
        .and_then(|p| p.0)
        .is_some_and(|addr| is_loopback_ip(addr.ip()))
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v.eq_ignore_ascii_case("true") || v == "1")
}

/// 静的トークン認証の判定(純粋関数。環境・リクエストに依存しないので
/// 単体テストできる)。`expected`は`OPEN_WEB_SERVER_ADMIN_TOKEN`。
///
/// **fail-closed**: トークン未設定なら拒否(503)。従来の無認証動作は
/// `OPEN_WEB_SERVER_ADMIN_INSECURE_DEV=true`を明示した開発時のみ。
fn static_auth_decision(
    mode: StaticTokenMode,
    expected: Option<&str>,
    insecure_dev: bool,
    provided: Option<&str>,
    loopback: bool,
) -> Result<(), (StatusCode, String)> {
    let Some(expected) = expected.filter(|e| !e.is_empty()) else {
        if insecure_dev {
            return Ok(());
        }
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "admin API is disabled: OPEN_WEB_SERVER_ADMIN_TOKEN is not set (set OPEN_WEB_SERVER_ADMIN_INSECURE_DEV=true for development only)".to_string(),
        ));
    };
    match mode {
        StaticTokenMode::Off => {
            return Err((StatusCode::FORBIDDEN, "static admin token is disabled (OPEN_WEB_SERVER_ADMIN_STATIC_TOKEN_MODE=off)".to_string()))
        }
        StaticTokenMode::Loopback if !loopback => {
            return Err((StatusCode::FORBIDDEN, "static admin token is accepted from loopback only".to_string()))
        }
        _ => {}
    }
    match provided {
        Some(token) if constant_time_eq(token, expected) => Ok(()),
        _ => Err((StatusCode::UNAUTHORIZED, format!("missing or invalid '{ADMIN_TOKEN_HEADER}' header"))),
    }
}

/// 静的共有シークレット認証。`OPEN_WEB_SERVER_ADMIN_STATIC_TOKEN_MODE`と
/// 送信元に従う。
fn check_static_admin_auth(req: &Request<Incoming>) -> Result<(), Response<BoxBody>> {
    let expected = std::env::var("OPEN_WEB_SERVER_ADMIN_TOKEN").ok();
    let provided = req.headers().get(ADMIN_TOKEN_HEADER).and_then(|v| v.to_str().ok());
    static_auth_decision(
        StaticTokenMode::from_env(),
        expected.as_deref(),
        env_flag("OPEN_WEB_SERVER_ADMIN_INSECURE_DEV"),
        provided,
        peer_is_loopback(req),
    )
    .map_err(|(status, msg)| text_response(status, msg))
}

/// 非常用(break-glass)認証: 静的トークン**かつループバック限定**
/// (modeが`any`でも常にループバックを要求する。Bearerは受け付けない)。
/// 2FA登録のリセットなど、2FA自体を迂回しうる操作に使う。
#[cfg(feature = "admin-2fa")]
pub(crate) fn check_breakglass_auth(req: &Request<Incoming>) -> Result<(), Response<BoxBody>> {
    let mode = match StaticTokenMode::from_env() {
        StaticTokenMode::Any => StaticTokenMode::Loopback,
        m => m,
    };
    let expected = std::env::var("OPEN_WEB_SERVER_ADMIN_TOKEN").ok();
    let provided = req.headers().get(ADMIN_TOKEN_HEADER).and_then(|v| v.to_str().ok());
    // 開発用の無認証オプトインはbreak-glassには適用しない(常にトークン必須)。
    static_auth_decision(mode, expected.as_deref(), false, provided, peer_is_loopback(req))
        .map_err(|(status, msg)| text_response(status, msg))
}

const TLS_TOKEN_HEADER: &str = "x-tls-token";

/// TLS登録専用トークン(`x-tls-token`、`OPEN_WEB_SERVER_TLS_TOKEN`)の判定。
/// 純粋関数。トークン未設定なら常に拒否、ループバック以外も拒否。
fn tls_token_decision(expected: Option<&str>, provided: &str, loopback: bool) -> Result<(), (StatusCode, String)> {
    let Some(expected) = expected.filter(|e| !e.is_empty()) else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "OPEN_WEB_SERVER_TLS_TOKEN is not set".to_string()));
    };
    if !loopback {
        return Err((StatusCode::FORBIDDEN, format!("'{TLS_TOKEN_HEADER}' is accepted from loopback only")));
    }
    if constant_time_eq(provided, expected) {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, format!("invalid '{TLS_TOKEN_HEADER}' header")))
    }
}

/// TLS証明書登録/削除エンドポイント専用の認証。`x-tls-token`が提示された
/// 場合はそれだけで判定する(間違っていても他の資格情報へフォールバック
/// しない)。無ければ通常の`check_admin_auth`(人用Bearer/静的トークン)。
/// このトークンで到達できるのは`POST/DELETE /admin/tenants/:host/tls`の
/// 2つだけ(他のハンドラは`check_admin_auth`を使うため通らない)。
pub(crate) fn check_tls_auth(state: &AppState, req: &Request<Incoming>) -> Result<(), Response<BoxBody>> {
    let Some(provided) = req.headers().get(TLS_TOKEN_HEADER).and_then(|v| v.to_str().ok()) else {
        return check_admin_auth(state, req);
    };
    let expected = std::env::var("OPEN_WEB_SERVER_TLS_TOKEN").ok();
    tls_token_decision(expected.as_deref(), provided, peer_is_loopback(req))
        .map_err(|(status, msg)| text_response(status, msg))
}

/// キーの`roles`が管理API(`/admin/*`全体・`/internal/*`)への
/// フルアクセスを許可するかどうか。**2026-07-30追記(セキュリティ強化、
/// roleベース権限分離)**: 従来は`KeyDecision::Ok`(=有効なキー)であれば
/// `owner`/`roles`を一切見ずに無条件でフルアクセスを許可していた——
/// 「developer向けに発行した1本のキー」も「真の管理者向けキー」も
/// 同じ権限を持ってしまう権限昇格に近い設計上の欠陥だった
/// (`CLAUDE.md`の2026-07-30続きエントリで発見・記録)。
///
/// **後方互換のための設計判断(ユーザー確認済み)**: `roles`が空
/// (`issue_key`呼び出し時に`roles`未指定、これまでの既定の発行方法)
/// のキーは**引き続きフルアクセス**として扱う——VPS等で既に運用中の
/// キーが今回の変更で突然ロックアウトされることを避けるため。
/// **今後、明示的に`roles`を指定して発行したキー**(例:
/// `["developer"]`のような制限付きロール)は、`"admin"`ロールを
/// 含んでいない限りフルアクセスを持たない——制限付きキーを新たに
/// 発行する場合にのみ、この制限が実際に効く設計。
fn key_grants_admin_access(roles: &[String]) -> bool {
    roles.is_empty() || roles.iter().any(|r| r == "admin")
}

/// 管理API向け認証。**「APIキーを意識しない仕様」の中核**:
/// `KeyGuardian`が発行した`Authorization: Bearer <key>`が検証に成功
/// し、かつそのキーの`roles`が管理アクセスを許可する場合(上記
/// `key_grants_admin_access`参照)にそれで通す(この経路を使う限り
/// 運用者は`OPEN_WEB_SERVER_ADMIN_TOKEN`という共有シークレットの
/// 存在自体を意識しなくてよい)。キーが無い・無効・失効済み・
/// 一時停止中・**または制限付きroleでフルアクセスを持たない**場合は、
/// 既存の静的共有シークレット(`x-admin-token`)へフォールバックする
/// ——最初のキーを発行する行為自体は、この静的シークレットで権限を
/// 持つ人が行う想定(ブートストラップの割り切り、`handlers::keys`の
/// doc comment参照)。
pub(crate) fn check_admin_auth(state: &AppState, req: &Request<Incoming>) -> Result<(), Response<BoxBody>> {
    // IPアドレス許可リスト(2026-07-30新設、`ip_allowlist`参照)。
    // `OPEN_WEB_SERVER_ADMIN_ALLOWED_IPS`が設定されている場合のみ有効
    // (既定は無効)。設定時は、正しいトークン/APIキーを持っていても
    // 許可リスト外の送信元からは管理APIへ到達できない——多層防御として
    // トークン検証より先に判定する。
    if let Some(allowlist) = crate::ip_allowlist::allowlist_from_env() {
        let peer_ip = req.extensions().get::<crate::PeerAddr>().and_then(|p| p.0).map(|addr| addr.ip());
        let allowed = peer_ip.is_some_and(|ip| crate::ip_allowlist::is_allowed(&allowlist, ip));
        if !allowed {
            return Err(text_response(
                StatusCode::FORBIDDEN,
                "source IP address is not on the admin allowlist (OPEN_WEB_SERVER_ADMIN_ALLOWED_IPS)",
            ));
        }
    }

    match crate::handlers::keys::check_bearer_key(state, req) {
        crate::keyring::KeyDecision::Ok { roles, .. } if key_grants_admin_access(&roles) => Ok(()),
        // 2026-07-30追記: 異常検知(`Suspended`)時、その`owner`が確認済み
        // TOTP経由の一時オーバーライド(`POST /admin/2fa/verify`)を
        // 持っていれば通す(`admin-2fa` feature有効時のみ)。無効時は
        // 従来通り常に拒否する。
        #[cfg(feature = "admin-2fa")]
        crate::keyring::KeyDecision::Suspended { owner } if state.two_factor.has_valid_override(&owner, chrono::Utc::now()) => Ok(()),
        crate::keyring::KeyDecision::Suspended { .. } => Err(text_response(
            StatusCode::TOO_MANY_REQUESTS,
            "API key temporarily suspended due to anomalous request rate\
             (if 2FA is enrolled for this owner, verify via POST /admin/2fa/verify to proceed)",
        )),
        // レジストリが空・未知のキー・キー未提示・制限付きroleで
        // フルアクセスを持たないキーのいずれの場合も、静的シークレット
        // へフォールバックする。
        crate::keyring::KeyDecision::RegistryEmpty
        | crate::keyring::KeyDecision::Rejected
        | crate::keyring::KeyDecision::Ok { .. } => check_static_admin_auth(req),
    }
}

/// `POST /admin/tenants` — ドメインを1件、無停止で追加する。
pub async fn add_tenant(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, &req) {
        return resp;
    }

    let config: TenantConfig = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    match state.tenants.add(config).await {
        Ok(()) => text_response(StatusCode::CREATED, "tenant added"),
        Err(TenantError::AlreadyExists(host)) => text_response(
            StatusCode::CONFLICT,
            format!("host '{host}' is already registered"),
        ),
        Err(e) => text_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// `PUT /admin/tenants/:host` — 既存ドメイン(またはサブドメイン)の設定を
/// 変更する。存在しない場合は`404`(誤って新規追加になってしまうのを防ぎ、
/// 追加は明示的に`POST /admin/tenants`を使わせるため)。
/// パス中の`:host`とボディの`host`が食い違う場合は`400`とし、
/// 意図しないホストの上書きを防ぐ。
pub async fn update_tenant(
    state: Arc<AppState>,
    req: Request<Incoming>,
    host: &str,
) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, &req) {
        return resp;
    }

    let config: TenantConfig = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    if config.host != host {
        return text_response(
            StatusCode::BAD_REQUEST,
            format!(
                "path host '{host}' does not match body host '{}'",
                config.host
            ),
        );
    }

    if !state.tenants.exists(host).await {
        return text_response(StatusCode::NOT_FOUND, format!("host '{host}' not found"));
    }

    state.tenants.upsert(config).await;
    text_response(StatusCode::OK, "tenant updated")
}

/// `DELETE /admin/tenants/:host` 相当(パスの末尾セグメントをhostとして扱う)。
/// 任意で `?path_prefix=/blog` クエリを付けると、そのHost配下の該当
/// `path_prefix`テナントのみを削除する(2026-07-22追記、"分身の術"の
/// 対象拡大)。クエリ省略時は従来通り`path_prefix`未指定のテナントを
/// 対象にする(後方互換)。
pub async fn remove_tenant(
    state: Arc<AppState>,
    req: &Request<Incoming>,
    host: &str,
) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }

    let path_prefix = req.uri().query().and_then(|q| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix("path_prefix="))
            .map(|v| urlencoding_lite_decode(v))
    });

    match state
        .tenants
        .remove_prefixed(host, path_prefix.as_deref())
        .await
    {
        Ok(()) => text_response(StatusCode::OK, "tenant removed"),
        Err(TenantError::NotFound(host)) => {
            text_response(StatusCode::NOT_FOUND, format!("host '{host}' not found"))
        }
        Err(e) => text_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// クエリパラメータ値の最小限のパーセントデコード(`%2F` → `/` 等)。
/// 本リポジトリは新規に`urlencoding`クレートへ依存させない方針のため、
/// この用途(`path_prefix`クエリ値のみ)に限定した最小実装とする。
fn urlencoding_lite_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `GET /admin/tenants` — 登録済みドメイン一覧。
pub async fn list_tenants(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }

    let list = state.tenants.list().await;
    json_response(StatusCode::OK, &list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parsing_defaults_to_any_and_unknown_falls_back_to_loopback() {
        assert_eq!(StaticTokenMode::parse(None), StaticTokenMode::Any);
        assert_eq!(StaticTokenMode::parse(Some("")), StaticTokenMode::Any);
        assert_eq!(StaticTokenMode::parse(Some("ANY")), StaticTokenMode::Any);
        assert_eq!(StaticTokenMode::parse(Some("loopback")), StaticTokenMode::Loopback);
        assert_eq!(StaticTokenMode::parse(Some(" off ")), StaticTokenMode::Off);
        assert_eq!(StaticTokenMode::parse(Some("lopback")), StaticTokenMode::Loopback);
    }

    #[test]
    fn loopback_detection_covers_v4_v6_and_mapped_addresses() {
        for ok in ["127.0.0.1", "127.9.9.9", "::1", "::ffff:127.0.0.1"] {
            assert!(is_loopback_ip(ok.parse().unwrap()), "{ok}");
        }
        for ng in ["10.0.0.1", "8.8.8.8", "2001:db8::1", "::ffff:8.8.8.8", "0.0.0.0"] {
            assert!(!is_loopback_ip(ng.parse().unwrap()), "{ng}");
        }
    }

    #[test]
    fn static_token_is_fail_closed_when_unset() {
        let r = static_auth_decision(StaticTokenMode::Any, None, false, None, true);
        assert_eq!(r.unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
        let r = static_auth_decision(StaticTokenMode::Any, Some(""), false, Some(""), true);
        assert_eq!(r.unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
        // 明示オプトインのときだけ従来の無認証。
        assert!(static_auth_decision(StaticTokenMode::Any, None, true, None, false).is_ok());
    }

    #[test]
    fn static_token_mode_any_accepts_remote_with_correct_token_only() {
        assert!(static_auth_decision(StaticTokenMode::Any, Some("s3"), false, Some("s3"), false).is_ok());
        assert_eq!(static_auth_decision(StaticTokenMode::Any, Some("s3"), false, Some("x"), false).unwrap_err().0, StatusCode::UNAUTHORIZED);
        assert_eq!(static_auth_decision(StaticTokenMode::Any, Some("s3"), false, None, true).unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn static_token_mode_loopback_rejects_remote_even_with_correct_token() {
        assert!(static_auth_decision(StaticTokenMode::Loopback, Some("s3"), false, Some("s3"), true).is_ok());
        assert_eq!(static_auth_decision(StaticTokenMode::Loopback, Some("s3"), false, Some("s3"), false).unwrap_err().0, StatusCode::FORBIDDEN);
        assert_eq!(static_auth_decision(StaticTokenMode::Loopback, Some("s3"), false, Some("bad"), true).unwrap_err().0, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn static_token_mode_off_rejects_everything() {
        assert_eq!(static_auth_decision(StaticTokenMode::Off, Some("s3"), false, Some("s3"), true).unwrap_err().0, StatusCode::FORBIDDEN);
    }

    #[test]
    fn tls_token_requires_configuration_loopback_and_exact_match() {
        assert!(tls_token_decision(Some("t"), "t", true).is_ok());
        assert_eq!(tls_token_decision(None, "t", true).unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(tls_token_decision(Some(""), "", true).unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(tls_token_decision(Some("t"), "t", false).unwrap_err().0, StatusCode::FORBIDDEN);
        assert_eq!(tls_token_decision(Some("t"), "wrong", true).unwrap_err().0, StatusCode::UNAUTHORIZED);
    }
}
