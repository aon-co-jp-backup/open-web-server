//! 2FAログインフロー(`admin-2fa` feature、2026-07-30新設)。
//!
//! ユーザー指示「常に二段階認証(メールOTP + QR/TOTP両方必須)」への
//! 対応。管理APIへの毎リクエストで2要素を都度要求すると、
//! `open-easy-web`等の機械的な呼び出し元(スクリプト・自動化ツール)が
//! 実用上使えなくなるため、**ログイン(セッション確立)時に2要素を
//! 要求し、成功したら短命の`KeyGuardian`Bearerキーを発行する**という
//! 実務的な設計にした(AWS/GCP等の実際の管理コンソールも「2FAは
//! ログイン時、以後はセッション/APIキー」という同じ設計)。
//!
//! フロー:
//! 1. 管理者(既存の`x-admin-token`を持つ者)が事前に`POST
//!    /admin/2fa/enroll`でTOTPを登録(QRコードをスマホの認証アプリで
//!    撮影)・`POST /admin/2fa/confirm`で初回コードを確認。
//! 2. 以後のログインは`POST /admin/login/request-otp`(メールOTP送信)→
//!    `POST /admin/login/verify`(メールOTP+TOTPコードの両方を提示)→
//!    成功すれば新しいBearerキー(既定2時間有効)を1回だけ返す。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;
use crate::two_factor::LoginCheck;

/// ログインキーの既定有効期間(分)。以前は12時間だったが窃取時の窓を
/// 狭めるため2時間に短縮。`OPEN_WEB_SERVER_LOGIN_KEY_TTL_MINUTES`で変更可
/// (5〜720分にクランプ)。
const DEFAULT_LOGIN_KEY_TTL_MINUTES: i64 = 120;

fn login_key_ttl_minutes(raw: Option<&str>) -> i64 {
    raw.and_then(|v| v.trim().parse::<i64>().ok())
        .map(|m| m.clamp(5, 720))
        .unwrap_or(DEFAULT_LOGIN_KEY_TTL_MINUTES)
}

#[derive(Debug, Deserialize)]
pub struct EnrollRequest {
    pub owner: String,
    pub email: Option<String>,
    pub phone_number: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EnrollResponse {
    pub owner: String,
    pub secret_base32: String,
    pub otpauth_url: String,
    pub qr_svg: Option<String>,
}

/// `POST /admin/2fa/enroll` — 管理者トークンを持つ者のみ実行できる
/// ブートストラップ操作(`handlers::tenants::check_admin_auth`と同じ
/// 権限を要求)。
pub async fn enroll(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = super::tenants::check_admin_auth(&state, &req) {
        return resp;
    }
    let body: EnrollRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if body.owner.trim().is_empty() {
        return text_response(StatusCode::BAD_REQUEST, "owner must not be empty");
    }
    let resp = match state.two_factor.enroll(&body.owner, body.email, body.phone_number) {
        Ok(r) => r,
        Err(crate::two_factor::EnrollError::AlreadyConfirmed) => {
            return text_response(
                StatusCode::CONFLICT,
                "2FA is already confirmed for this owner; re-enrolment requires POST /admin/2fa/reset (loopback + static admin token)",
            )
        }
    };
    json_response(
        StatusCode::CREATED,
        &EnrollResponse { owner: resp.owner, secret_base32: resp.secret_base32, otpauth_url: resp.otpauth_url, qr_svg: resp.qr_svg },
    )
}

#[derive(Debug, Deserialize)]
pub struct ResetRequest {
    pub owner: String,
}

/// `POST /admin/2fa/reset` — 登録(確認済みを含む)を削除する。2FA自体を
/// 迂回しうる操作なので、**break-glass認証(静的トークン+ループバック
/// 限定)のみ**を受け付け、人用のBearerキーでは実行できない。
pub async fn reset_enrollment(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = super::tenants::check_breakglass_auth(&req) {
        return resp;
    }
    let body: ResetRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if state.two_factor.reset(&body.owner) {
        json_response(StatusCode::OK, &serde_json::json!({ "reset": true }))
    } else {
        text_response(StatusCode::NOT_FOUND, "no 2FA enrolment for this owner")
    }
}

#[derive(Debug, Deserialize)]
pub struct ConfirmRequest {
    pub owner: String,
    pub code: String,
}

/// `POST /admin/2fa/confirm` — 登録直後の初回コード確認。
pub async fn confirm(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = super::tenants::check_admin_auth(&state, &req) {
        return resp;
    }
    let body: ConfirmRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if state.two_factor.confirm(&body.owner, &body.code, chrono::Utc::now()) {
        json_response(StatusCode::OK, &serde_json::json!({ "confirmed": true }))
    } else {
        text_response(StatusCode::BAD_REQUEST, "invalid or expired TOTP code")
    }
}

#[derive(Debug, Deserialize)]
pub struct RequestOtpRequest {
    pub owner: String,
}

/// `POST /admin/login/request-otp` — **認証不要**(これ自体がログイン
/// フローの入口のため)。存在しない/未登録の`owner`でも常に同じ`200`を
/// 返す(owner列挙攻撃を避けるため、実際にメールが送られたかどうかを
/// レスポンスから判別できないようにする)。
pub async fn request_otp(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    let peer_ip = req
        .extensions()
        .get::<crate::PeerAddr>()
        .and_then(|p| p.0)
        .map(|a| a.ip().to_canonical().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let body: RequestOtpRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    let now = chrono::Utc::now();
    // owner+IPごとのレート制限。登録済みか否かに関わらず同じ制限を課す
    // ので、429の有無からownerの存在は分からない。
    if !state.two_factor.allow_otp_request(&body.owner, &peer_ip, now) {
        return text_response(StatusCode::TOO_MANY_REQUESTS, "too many OTP requests; try again later");
    }

    // 有効なチャレンジがある間は再発行しない(正規ユーザーのOTPを第三者が
    // 上書き・無効化し続けるログイン妨害と、メール爆撃を防ぐ)。
    if state.two_factor.has_active_email_challenge(&body.owner, now) {
        return json_response(StatusCode::OK, &serde_json::json!({ "message": "if this owner has 2FA enrolled, an OTP email has been sent" }));
    }

    if let Some(email) = state.two_factor.email_for(&body.owner) {
        if let Some(smtp_config) = crate::two_factor::SmtpConfig::from_env() {
            let code = state.two_factor.issue_email_otp(&body.owner, now);
            let two_factor = state.two_factor.clone();
            let owner = body.owner.clone();
            // メール送信(同期SMTP)はブロッキングI/Oのため、リクエスト
            // ハンドラのtokioワーカースレッドを塞がないよう
            // `spawn_blocking`へオフロードする(rs-syncの`send_otp`と
            // 同じ設計)。失敗してもログのみ——列挙攻撃を避けるため
            // レスポンス自体は既に確定した`200`のまま変えない。
            tokio::task::spawn_blocking(move || {
                if let Err(e) = crate::two_factor::send_otp_email(&smtp_config, &email, &code) {
                    tracing::warn!(owner = %owner, error = %e, "failed to send admin login OTP email");
                }
            });
            let _ = two_factor; // keep the Arc alive for the closure's lifetime clarity
        } else {
            tracing::warn!("admin login OTP requested but OPEN_WEB_SERVER_SMTP_* is not fully configured");
        }
    }

    json_response(StatusCode::OK, &serde_json::json!({ "message": "if this owner has 2FA enrolled, an OTP email has been sent" }))
}

#[derive(Debug, Deserialize)]
pub struct SuspensionOverrideRequest {
    pub owner: String,
    pub totp_code: String,
}

/// `POST /admin/2fa/verify` — **既に有効なBearerキーを持っている
/// owner**が、`KeyGuardian`の異常検知(`KeyDecision::Suspended`)で
/// 一時停止された際に、TOTPコード単体で一時的な通過オーバーライドを
/// 得るための経路(`POST /admin/login/verify`のフルログインとは別軸
/// ——こちらは「元々有効なキーを持っている人が、異常検知の隔離期間を
/// 早く抜けたい」場合向け、フルログインは「そもそもキーを持っていない」
/// 場合向け)。認証は不要(TOTPコード自体が確認済み登録者の証明になる)。
pub async fn verify_suspension_override(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    let body: SuspensionOverrideRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    match state.two_factor.verify_for_override(&body.owner, &body.totp_code, chrono::Utc::now()) {
        LoginCheck::Success => json_response(StatusCode::OK, &serde_json::json!({ "override_granted": true })),
        LoginCheck::Locked => text_response(StatusCode::TOO_MANY_REQUESTS, "too many failed attempts; locked temporarily"),
        LoginCheck::Invalid => text_response(StatusCode::UNAUTHORIZED, "invalid TOTP code, or 2FA not confirmed for this owner"),
    }
}

#[derive(Debug, Deserialize)]
pub struct VerifyLoginRequest {
    pub owner: String,
    pub email_otp: String,
    pub totp_code: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyLoginResponse {
    pub key: String,
    pub owner: String,
    pub expires_in_minutes: i64,
}

/// `POST /admin/login/verify` — **認証不要**(ログインフローの本体)。
/// メールOTP・TOTPコードの両方が正しい場合のみ、既定2時間有効の
/// `KeyGuardian`Bearerキーを新規発行して返す(以後の管理API呼び出しは
/// このキーを`Authorization: Bearer`で使う、既存のキー方式をそのまま
/// 再利用)。
pub async fn verify_login(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    let body: VerifyLoginRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    let now = chrono::Utc::now();
    match state.two_factor.verify_login(&body.owner, &body.email_otp, &body.totp_code, now) {
        LoginCheck::Success => {}
        LoginCheck::Locked => return text_response(StatusCode::TOO_MANY_REQUESTS, "too many failed attempts; locked temporarily"),
        LoginCheck::Invalid => {
            return text_response(StatusCode::UNAUTHORIZED, "email OTP and/or TOTP code invalid, expired, or 2FA not enrolled for this owner")
        }
    }

    let ttl_minutes = login_key_ttl_minutes(std::env::var("OPEN_WEB_SERVER_LOGIN_KEY_TTL_MINUTES").ok().as_deref());
    let expires_at = now + chrono::Duration::minutes(ttl_minutes);
    let key = state.keyring.issue(&body.owner, vec!["admin".to_string()], Some(expires_at));

    json_response(
        StatusCode::OK,
        &VerifyLoginResponse { key, owner: body.owner, expires_in_minutes: ttl_minutes },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_key_ttl_defaults_to_two_hours_and_clamps() {
        assert_eq!(login_key_ttl_minutes(None), 120);
        assert_eq!(login_key_ttl_minutes(Some("abc")), 120);
        assert_eq!(login_key_ttl_minutes(Some("30")), 30);
        assert_eq!(login_key_ttl_minutes(Some("1")), 5);
        assert_eq!(login_key_ttl_minutes(Some("100000")), 720);
    }
}
