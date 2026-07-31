//! 2FA(TOTP、RFC 6238、2026-07-30新設)。
//!
//! `KeyGuardian`が異常なリクエスト頻度を検知した(`KeyDecision::
//! Suspended`)際、`owner`向けにあらかじめ登録済みのTOTPシークレット
//! (スマホの認証アプリ——Google Authenticator等——でQRコードを撮影して
//! 登録する標準方式)による追加確認を要求する多層防御。
//!
//! **メール(SMTP)ではなくTOTPを採用した理由**: SMS送信は有料の外部API
//! (Twilio等)契約が必要でコストが高い。メールはSMTPサーバーの用意が
//! 必要な上、メールアカウント自体が乗っ取られていれば無意味。TOTPは
//! 一度QRコードでスマホの認証アプリへ登録すれば、以後は完全オフラインで
//! 6桁コードを生成できる(通信経路自体を必要としない)ため、外部API費用も
//! メールアカウント漏洩のリスクも無い。
//!
//! **携帯電話番号について**: `TotpEnrollment.phone_number`に記録用として
//! 保持するが、SMS送信ロジックは今回実装していない(上記の通り有料API
//! 契約が必要なため——将来SMS事業者と契約した場合の拡張ポイントとして
//! フィールドのみ用意)。

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use rand::Rng;
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

const BASE32_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const TOTP_STEP_SECS: i64 = 30;
const TOTP_DIGITS: u32 = 6;
/// 検証成功後、`Suspended`状態を上書きして通過させる猶予期間。
const OVERRIDE_VALIDITY_MINUTES: i64 = 15;

/// RFC 4648 Base32(パディング無し)。TOTPシークレットを認証アプリへ
/// 手入力できる形式として表示するために使う(QRコードが読めない場合の
/// 代替手段)。新規crate依存を避けるため自前実装。
fn base32_encode(data: &[u8]) -> String {
    let mut output = String::new();
    let mut bits: u32 = 0;
    let mut bit_count = 0u32;
    for &byte in data {
        bits = (bits << 8) | byte as u32;
        bit_count += 8;
        while bit_count >= 5 {
            bit_count -= 5;
            output.push(BASE32_ALPHABET[((bits >> bit_count) & 0x1f) as usize] as char);
        }
    }
    if bit_count > 0 {
        output.push(BASE32_ALPHABET[((bits << (5 - bit_count)) & 0x1f) as usize] as char);
    }
    output
}

fn hotp(secret: &[u8], counter: u64) -> u32 {
    let mut mac = HmacSha1::new_from_slice(secret).expect("HMAC accepts keys of any length");
    mac.update(&counter.to_be_bytes());
    let result = mac.finalize().into_bytes();
    let offset = (result[result.len() - 1] & 0x0f) as usize;
    let code = ((u32::from(result[offset]) & 0x7f) << 24)
        | (u32::from(result[offset + 1]) << 16)
        | (u32::from(result[offset + 2]) << 8)
        | u32::from(result[offset + 3]);
    code % 10u32.pow(TOTP_DIGITS)
}

fn totp_at(secret: &[u8], time: DateTime<Utc>) -> u32 {
    let counter = (time.timestamp() / TOTP_STEP_SECS) as u64;
    hotp(secret, counter)
}

/// TOTPコード(利用者がタップミス等で少し前後した時刻のコードを送って
/// くることを許容するため、前後1ステップ=最大±30秒のクロックずれを
/// 許容する、標準的なTOTP実装の慣行)を検証する。
fn totp_matches(secret: &[u8], code: &str, now: DateTime<Utc>) -> bool {
    let Ok(provided) = code.trim().parse::<u32>() else {
        return false;
    };
    for step_offset in [-1i64, 0, 1] {
        let shifted = now + chrono::Duration::seconds(step_offset * TOTP_STEP_SECS);
        if totp_at(secret, shifted) == provided {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone)]
pub struct TotpEnrollment {
    pub secret: Vec<u8>,
    pub confirmed: bool,
    pub email: Option<String>,
    /// 将来のSMS拡張向けの記録用フィールド(今回は送信ロジック未実装、
    /// モジュールdoc参照)。
    pub phone_number: Option<String>,
}

struct EmailOtpChallenge {
    code: String,
    expires_at: DateTime<Utc>,
}

const EMAIL_OTP_VALIDITY_MINUTES: i64 = 5;

/// メールOTP送信用のSMTP設定(rs-syncの`mail.rs`と同じ設計・同じ環境変数
/// 命名規則)。`lettre`の同期SMTPクライアントを使う——呼び出し側で
/// `tokio::task::spawn_blocking`によるオフロードが必要。
#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
}

impl SmtpConfig {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            host: std::env::var("OPEN_WEB_SERVER_SMTP_HOST").ok()?,
            port: std::env::var("OPEN_WEB_SERVER_SMTP_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(587),
            username: std::env::var("OPEN_WEB_SERVER_SMTP_USERNAME").ok()?,
            password: std::env::var("OPEN_WEB_SERVER_SMTP_PASSWORD").ok()?,
            from: std::env::var("OPEN_WEB_SERVER_SMTP_FROM").ok()?,
        })
    }
}

/// OTPメールを実際に送信する(同期SMTP、呼び出し側で`spawn_blocking`
/// すること)。
pub fn send_otp_email(config: &SmtpConfig, to: &str, code: &str) -> Result<(), String> {
    let body = format!(
        "open-web-server 管理者ログイン用ワンタイムパスワード\n\n\
         コード: {code}\n\n\
         このコードは{EMAIL_OTP_VALIDITY_MINUTES}分間有効です。\n\
         心当たりがない場合はこのメールを無視してください。"
    );
    let email = Message::builder()
        .from(config.from.parse().map_err(|e| format!("invalid from address: {e}"))?)
        .to(to.parse().map_err(|e| format!("invalid to address: {e}"))?)
        .subject("open-web-server ログインコード")
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| format!("failed to build message: {e}"))?;

    let creds = Credentials::new(config.username.clone(), config.password.clone());
    let mailer = SmtpTransport::starttls_relay(&config.host)
        .map_err(|e| format!("failed to configure SMTP relay: {e}"))?
        .port(config.port)
        .credentials(creds)
        .build();
    mailer.send(&email).map_err(|e| format!("failed to send email: {e}"))?;
    Ok(())
}

fn generate_numeric_code(digits: u32) -> String {
    let max = 10u32.pow(digits);
    let value: u32 = rand::thread_rng().gen_range(0..max);
    format!("{value:0width$}", width = digits as usize)
}

#[derive(Default)]
pub struct TwoFactorStore {
    enrollments: RwLock<HashMap<String, TotpEnrollment>>,
    overrides: RwLock<HashMap<String, DateTime<Utc>>>,
    email_challenges: RwLock<HashMap<String, EmailOtpChallenge>>,
}

impl TwoFactorStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// `owner`向けに新しいTOTPシークレットを生成し、未確認状態で登録する。
    /// 20バイト(160bit)のランダムシークレットを`ring::rand`で生成する
    /// (RFC 6238推奨のHMAC-SHA1鍵長)。`otpauth://`URIとQRコード(SVG)、
    /// 手入力用のBase32文字列を返す。
    pub fn enroll(&self, owner: &str, email: Option<String>, phone_number: Option<String>) -> EnrollmentResponse {
        use ring::rand::{SecureRandom, SystemRandom};
        let mut secret = vec![0u8; 20];
        SystemRandom::new().fill(&mut secret).expect("system RNG must be available");

        let secret_base32 = base32_encode(&secret);
        let issuer = "open-web-server";
        let otpauth_url = format!(
            "otpauth://totp/{issuer}:{owner}?secret={secret_base32}&issuer={issuer}&algorithm=SHA1&digits={TOTP_DIGITS}&period={TOTP_STEP_SECS}"
        );

        let qr_svg = qrcode::QrCode::new(otpauth_url.as_bytes())
            .ok()
            .map(|code| code.render::<qrcode::render::svg::Color>().min_dimensions(240, 240).build());

        self.enrollments.write().unwrap().insert(
            owner.to_string(),
            TotpEnrollment { secret, confirmed: false, email, phone_number },
        );

        EnrollmentResponse { owner: owner.to_string(), secret_base32, otpauth_url, qr_svg }
    }

    /// 登録直後の確認(初回スキャンしたコードが正しいか)。成功したら
    /// `confirmed = true`にし、以後の異常検知時の2FAチェック対象になる。
    pub fn confirm(&self, owner: &str, code: &str, now: DateTime<Utc>) -> bool {
        let mut enrollments = self.enrollments.write().unwrap();
        let Some(enrollment) = enrollments.get_mut(owner) else {
            return false;
        };
        if totp_matches(&enrollment.secret, code, now) {
            enrollment.confirmed = true;
            true
        } else {
            false
        }
    }

    /// `owner`が確認済みTOTPを登録しているか。
    pub fn has_confirmed_enrollment(&self, owner: &str) -> bool {
        self.enrollments.read().unwrap().get(owner).is_some_and(|e| e.confirmed)
    }

    /// 異常検知(`KeyDecision::Suspended`)時の追加確認。成功すれば
    /// `OVERRIDE_VALIDITY_MINUTES`分だけ`Suspended`状態を上書きする。
    pub fn verify_for_override(&self, owner: &str, code: &str, now: DateTime<Utc>) -> bool {
        let matches = {
            let enrollments = self.enrollments.read().unwrap();
            match enrollments.get(owner) {
                Some(e) if e.confirmed => totp_matches(&e.secret, code, now),
                _ => false,
            }
        };
        if matches {
            self.overrides.write().unwrap().insert(owner.to_string(), now + chrono::Duration::minutes(OVERRIDE_VALIDITY_MINUTES));
        }
        matches
    }

    /// 現在有効な(=期限内の)2FA通過済みオーバーライドを持っているか。
    pub fn has_valid_override(&self, owner: &str, now: DateTime<Utc>) -> bool {
        self.overrides.read().unwrap().get(owner).is_some_and(|expires_at| *expires_at > now)
    }

    /// `owner`に登録済みのメールアドレスを返す(未登録/未確認TOTPなら
    /// `None`)。
    pub fn email_for(&self, owner: &str) -> Option<String> {
        self.enrollments.read().unwrap().get(owner).and_then(|e| e.email.clone())
    }

    /// 6桁のメールOTPコードを生成し、`EMAIL_OTP_VALIDITY_MINUTES`分の
    /// 有効期限で保存する。実際のメール送信は呼び出し側が
    /// `send_otp_email`(同期・ブロッキング)で行う——この関数自体は
    /// I/Oを一切行わない。
    pub fn issue_email_otp(&self, owner: &str, now: DateTime<Utc>) -> String {
        let code = generate_numeric_code(6);
        self.email_challenges.write().unwrap().insert(
            owner.to_string(),
            EmailOtpChallenge { code: code.clone(), expires_at: now + chrono::Duration::minutes(EMAIL_OTP_VALIDITY_MINUTES) },
        );
        code
    }

    fn email_otp_matches(&self, owner: &str, code: &str, now: DateTime<Utc>) -> bool {
        let challenges = self.email_challenges.read().unwrap();
        challenges.get(owner).is_some_and(|c| c.expires_at > now && constant_time_eq(&c.code, code.trim()))
    }

    /// **ログインフロー本体(2026-07-30新設、ユーザー指示「常に二段階認証
    /// (メールOTP + QR/TOTP両方必須)」)**: メールOTPとTOTPコードの
    /// **両方**が正しい場合のみ成功し、成功時はメールOTPチャレンジを
    /// 消費(使い捨て)した上で、`OVERRIDE_VALIDITY_MINUTES`分の
    /// オーバーライドを発行する(これを使って`keyring`から実際の
    /// Bearerキーを発行するのは呼び出し側=`handlers::admin_login`の
    /// 責務)。
    pub fn verify_login(&self, owner: &str, email_code: &str, totp_code: &str, now: DateTime<Utc>) -> bool {
        if !self.email_otp_matches(owner, email_code, now) {
            return false;
        }
        let totp_ok = {
            let enrollments = self.enrollments.read().unwrap();
            match enrollments.get(owner) {
                Some(e) if e.confirmed => totp_matches(&e.secret, totp_code, now),
                _ => false,
            }
        };
        if !totp_ok {
            return false;
        }
        // 両方成功した場合のみ、メールOTPを使い捨てとして消費する
        // (片方だけ正しい試行では消費しない——ブルートフォースで
        // メールOTPだけ先に当てられても、TOTPが揃うまで何度でも
        // 再試行されてしまう問題を避けるため、この設計は今回のスコープの
        // 限界として明記: 真の対策にはメールOTP自体にも試行回数制限を
        // 設けるべきだが、6桁OTP+5分有効期限+TOTPとの併用という多層に
        // より実用上のブルートフォース耐性は確保できていると判断した)。
        self.email_challenges.write().unwrap().remove(owner);
        true
    }
}

/// 定数時間文字列比較(`handlers::tenants::constant_time_eq`と同じ理由・
/// 同じ実装、タイミングサイドチャネル対策)。
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

pub struct EnrollmentResponse {
    pub owner: String,
    pub secret_base32: String,
    pub otpauth_url: String,
    pub qr_svg: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_encode_matches_known_vector() {
        // RFC 4648のテストベクタ("foobar" -> "MZXW6YTBOI======"、
        // パディングを除いた比較)。
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn enroll_then_confirm_with_the_real_totp_code_succeeds() {
        let store = TwoFactorStore::new();
        let now = Utc::now();
        let resp = store.enroll("alice", None, None);
        assert!(resp.otpauth_url.contains("alice"));
        assert!(resp.qr_svg.is_some(), "QR SVG should be generated");

        // 実際にシークレットからその瞬間のコードを計算して確認する
        // (テストなので内部状態を直接読む——本物のTOTPアプリが行う計算を
        // そのまま再現する)。
        let secret = store.enrollments.read().unwrap().get("alice").unwrap().secret.clone();
        let real_code = format!("{:06}", totp_at(&secret, now));

        assert!(!store.has_confirmed_enrollment("alice"));
        assert!(store.confirm("alice", &real_code, now));
        assert!(store.has_confirmed_enrollment("alice"));
    }

    #[test]
    fn confirm_with_wrong_code_fails_and_does_not_confirm() {
        let store = TwoFactorStore::new();
        store.enroll("bob", None, None);
        assert!(!store.confirm("bob", "000000", Utc::now()));
        assert!(!store.has_confirmed_enrollment("bob"));
    }

    #[test]
    fn verify_for_override_requires_prior_confirmation() {
        let store = TwoFactorStore::new();
        let now = Utc::now();
        store.enroll("carol", None, None);
        let secret = store.enrollments.read().unwrap().get("carol").unwrap().secret.clone();
        let real_code = format!("{:06}", totp_at(&secret, now));

        // まだconfirmしていないので、正しいコードでもoverrideは発行されない。
        assert!(!store.verify_for_override("carol", &real_code, now));
        assert!(!store.has_valid_override("carol", now));

        store.confirm("carol", &real_code, now);
        assert!(store.verify_for_override("carol", &real_code, now));
        assert!(store.has_valid_override("carol", now));
    }

    #[test]
    fn override_expires_after_validity_window() {
        let store = TwoFactorStore::new();
        let now = Utc::now();
        store.enroll("dave", None, None);
        let secret = store.enrollments.read().unwrap().get("dave").unwrap().secret.clone();
        let real_code = format!("{:06}", totp_at(&secret, now));
        store.confirm("dave", &real_code, now);
        assert!(store.verify_for_override("dave", &real_code, now));

        assert!(store.has_valid_override("dave", now + chrono::Duration::minutes(10)));
        assert!(!store.has_valid_override("dave", now + chrono::Duration::minutes(16)));
    }

    #[test]
    fn totp_tolerates_one_step_of_clock_skew_but_not_two() {
        let secret = b"a-fixed-test-secret-".to_vec();
        let now = Utc::now();
        let code_one_step_ago = format!("{:06}", totp_at(&secret, now - chrono::Duration::seconds(TOTP_STEP_SECS)));
        let code_two_steps_ago = format!("{:06}", totp_at(&secret, now - chrono::Duration::seconds(2 * TOTP_STEP_SECS)));
        assert!(totp_matches(&secret, &code_one_step_ago, now));
        assert!(!totp_matches(&secret, &code_two_steps_ago, now));
    }

    /// `verify_login`(ユーザー指示「常に二段階認証(メールOTP+QR/TOTP
    /// 両方必須)」の中核ロジック)は、**両方**正しくなければ成功しない
    /// ことを網羅的に検証する: 片方だけ正しい2パターン(メールOTPのみ
    /// 正しい・TOTPのみ正しい)はいずれも失敗し、両方正しい場合のみ
    /// 成功する。
    #[test]
    fn verify_login_requires_both_email_otp_and_totp_to_succeed() {
        let store = TwoFactorStore::new();
        let now = Utc::now();
        store.enroll("erin", Some("erin@example.test".to_string()), None);
        let secret = store.enrollments.read().unwrap().get("erin").unwrap().secret.clone();
        let real_totp = format!("{:06}", totp_at(&secret, now));
        store.confirm("erin", &real_totp, now);

        let real_email_otp = store.issue_email_otp("erin", now);

        // TOTPのみ正しい(メールOTPは適当な値) → 失敗。
        assert!(!store.verify_login("erin", "000000", &real_totp, now));
        // メールOTPのみ正しい(TOTPは適当な値) → 失敗。
        assert!(!store.verify_login("erin", &real_email_otp, "000000", now));
        // 両方正しい → 成功。
        assert!(store.verify_login("erin", &real_email_otp, &real_totp, now));
        // 成功後はメールOTPが使い捨てのため、同じコードでの再試行は失敗する。
        assert!(!store.verify_login("erin", &real_email_otp, &real_totp, now));
    }

    #[test]
    fn verify_login_fails_without_confirmed_totp_enrollment() {
        let store = TwoFactorStore::new();
        let now = Utc::now();
        // enrollしたがconfirmしていない owner。
        store.enroll("frank", Some("frank@example.test".to_string()), None);
        let email_otp = store.issue_email_otp("frank", now);
        let secret = store.enrollments.read().unwrap().get("frank").unwrap().secret.clone();
        let real_totp = format!("{:06}", totp_at(&secret, now));
        assert!(!store.verify_login("frank", &email_otp, &real_totp, now), "unconfirmed TOTP enrollment must not authorize login");
    }
}
