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
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use rand::Rng;
use serde::{Deserialize, Serialize};
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
/// 許容する、標準的なTOTP実装の慣行)を検証し、一致した**ステップ番号**
/// (Unix秒/30)を返す。呼び出し側は直近に受理したステップ以下を拒否
/// することでコードの再利用(リプレイ)を防ぐ。
fn totp_matching_step(secret: &[u8], code: &str, now: DateTime<Utc>) -> Option<i64> {
    let provided = code.trim().parse::<u32>().ok()?;
    for step_offset in [-1i64, 0, 1] {
        let shifted = now + chrono::Duration::seconds(step_offset * TOTP_STEP_SECS);
        if totp_at(secret, shifted) == provided {
            return Some(shifted.timestamp() / TOTP_STEP_SECS);
        }
    }
    None
}

/// 直近受理ステップより新しいステップで一致した場合のみ、そのステップを返す。
fn fresh_totp_step(e: &TotpEnrollment, code: &str, now: DateTime<Utc>) -> Option<i64> {
    totp_matching_step(&e.secret, code, now).filter(|s| e.last_used_step.is_none_or(|last| *s > last))
}

/// メールOTP要求のレート制限(owner+IPごと)。
const OTP_REQUEST_LIMIT: usize = 3;
const OTP_REQUEST_WINDOW_SECS: i64 = 300;
/// 連続失敗の上限とロック時間(ownerごと)。
const MAX_FAILED_ATTEMPTS: u32 = 5;
const LOCKOUT_MINUTES: i64 = 15;
/// 追跡テーブルの最大件数(認証不要エンドポイントからのメモリ枯渇対策)。
const MAX_TRACKED_KEYS: usize = 10_000;

/// ログイン/オーバーライド検証の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginCheck {
    Success,
    Invalid,
    /// 連続失敗によりロック中(正しいコードでも拒否する)。
    Locked,
}

impl LoginCheck {
    #[cfg(test)]
    pub fn is_success(self) -> bool {
        self == Self::Success
    }
}

#[derive(Debug)]
pub enum EnrollError {
    /// 確認済み(`confirmed`)の登録が既にある。再登録は明示的なresetが必要。
    AlreadyConfirmed,
}

#[derive(Debug, Clone, Copy)]
struct FailureState {
    failures: u32,
    window_start: DateTime<Utc>,
    locked_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotpEnrollment {
    pub secret: Vec<u8>,
    pub confirmed: bool,
    pub email: Option<String>,
    /// 将来のSMS拡張向けの記録用フィールド(今回は送信ロジック未実装、
    /// モジュールdoc参照)。
    pub phone_number: Option<String>,
    /// 最後に受理したTOTPステップ(再利用防止)。旧ファイルには無いので
    /// 欠落時は`None`。
    #[serde(default)]
    pub last_used_step: Option<i64>,
}

/// TOTP登録済みシークレットを`path`へアトミックに書き込む(`keyring.rs`の
/// `write_records_atomically`と同じ「一時ファイル→rename」パターン——
/// 書き込み途中のクラッシュで永続化ファイルが破損しないようにするため)。
/// **正直な開示**: シークレット自体は平文でディスクに書かれる
/// (`KeyGuardian`がキーをハッシュのみ保存するのとは異なり、TOTP検証には
/// シークレット原文が必要なため)。ファイルシステム自体のアクセス権限
/// (ホストOSの権限分離)が最終防衛線になる設計であることを明記する。
fn write_enrollments_atomically(path: &Path, enrollments: &HashMap<String, TotpEnrollment>) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(enrollments).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            create_private_dir_all(parent)?;
        }
    }
    let tmp_path = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or("json"),
        std::process::id()
    ));
    write_private_file(&tmp_path, json.as_bytes())?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// 所有者のみ読み書き可(Unixでは0600)でファイルを作成して書き込む。
/// TOTPシークレットは平文で保存されるため、umask任せにしない。
fn write_private_file(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(contents)?;
    f.sync_all()
}

/// 親ディレクトリを(無ければ)作成する。新規作成したディレクトリは
/// Unixでは0700にする(既存ディレクトリの権限は勝手に変えない)。
fn create_private_dir_all(dir: &Path) -> std::io::Result<()> {
    let existed = dir.exists();
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = existed;
    Ok(())
}

fn load_enrollments_from_path(path: &Path) -> HashMap<String, TotpEnrollment> {
    match std::fs::read_to_string(path) {
        Ok(contents) => match sonic_rs::from_str(&contents) {
            Ok(map) => map,
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "TwoFactorStore: failed to parse persisted enrollments, starting empty");
                HashMap::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "TwoFactorStore: failed to read persisted enrollments, starting empty");
            HashMap::new()
        }
    }
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
    /// `owner\nip`ごとのメールOTP要求時刻(レート制限用、メモリ上のみ)。
    otp_requests: Mutex<HashMap<String, Vec<DateTime<Utc>>>>,
    /// ownerごとの連続失敗/ロック状態(メモリ上のみ)。
    failures: Mutex<HashMap<String, FailureState>>,
    persistence_path: Option<PathBuf>,
}

impl TwoFactorStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// `OPEN_WEB_SERVER_2FA_STORE_PATH`が設定されていれば、そのファイルから
    /// TOTP登録済み情報を復元して起動する(`KeyGuardian::load_from_disk`と
    /// 同じ「補助系機能の失敗は本体起動を止めない」設計方針——ファイルが
    /// 無い・読めない・壊れているいずれの場合もパニックせず空の状態から
    /// 開始する)。これが無いと、プロセス再起動のたびに全ユーザーが
    /// TOTP再登録(QRコード再撮影)を強いられてしまう。
    pub fn load_from_env() -> Self {
        let persistence_path = std::env::var("OPEN_WEB_SERVER_2FA_STORE_PATH").ok().map(PathBuf::from);
        let enrollments = match &persistence_path {
            Some(path) => load_enrollments_from_path(path),
            None => HashMap::new(),
        };
        Self { enrollments: RwLock::new(enrollments), persistence_path, ..Self::default() }
    }

    fn persist(&self, enrollments: &HashMap<String, TotpEnrollment>) {
        let Some(path) = &self.persistence_path else {
            return;
        };
        if let Err(e) = write_enrollments_atomically(path, enrollments) {
            tracing::warn!(error = %e, path = %path.display(), "TwoFactorStore: failed to persist enrollments to disk");
        }
    }

    /// `owner`向けに新しいTOTPシークレットを生成し、未確認状態で登録する。
    /// 20バイト(160bit)のランダムシークレットを`ring::rand`で生成する
    /// (RFC 6238推奨のHMAC-SHA1鍵長)。`otpauth://`URIとQRコード(SVG)、
    /// 手入力用のBase32文字列を返す。
    ///
    /// **確認済み(`confirmed`)のownerへの再登録は拒否する**(静的トークン
    /// 保持者による秘密鍵の上書き=2FA迂回を防ぐ)。再登録したい場合は
    /// 先に`reset`(break-glass認証必須)で削除する。未確認の登録は上書き可。
    pub fn enroll(&self, owner: &str, email: Option<String>, phone_number: Option<String>) -> Result<EnrollmentResponse, EnrollError> {
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

        let mut enrollments = self.enrollments.write().unwrap();
        if enrollments.get(owner).is_some_and(|e| e.confirmed) {
            return Err(EnrollError::AlreadyConfirmed);
        }
        enrollments.insert(owner.to_string(), TotpEnrollment { secret, confirmed: false, email, phone_number, last_used_step: None });
        self.persist(&enrollments);
        drop(enrollments);

        Ok(EnrollmentResponse { owner: owner.to_string(), secret_base32, otpauth_url, qr_svg })
    }

    /// `owner`の登録(確認済みを含む)を削除する。呼び出し側(break-glass
    /// 認証済みのハンドラ)の責務で認可すること。削除したら`true`。
    pub fn reset(&self, owner: &str) -> bool {
        let mut enrollments = self.enrollments.write().unwrap();
        let removed = enrollments.remove(owner).is_some();
        if removed {
            self.persist(&enrollments);
        }
        drop(enrollments);
        self.overrides.write().unwrap().remove(owner);
        self.email_challenges.write().unwrap().remove(owner);
        self.failures.lock().unwrap().remove(owner);
        removed
    }

    /// 登録直後の確認(初回スキャンしたコードが正しいか)。成功したら
    /// `confirmed = true`にし、以後の異常検知時の2FAチェック対象になる。
    /// 確認に使ったTOTPステップは再利用できない。
    pub fn confirm(&self, owner: &str, code: &str, now: DateTime<Utc>) -> bool {
        let mut enrollments = self.enrollments.write().unwrap();
        let Some(enrollment) = enrollments.get_mut(owner) else {
            return false;
        };
        if let Some(step) = fresh_totp_step(enrollment, code, now) {
            enrollment.confirmed = true;
            enrollment.last_used_step = Some(step);
            self.persist(&enrollments);
            true
        } else {
            false
        }
    }

    /// `owner`が確認済みTOTPを登録しているか。
    pub fn has_confirmed_enrollment(&self, owner: &str) -> bool {
        self.enrollments.read().unwrap().get(owner).is_some_and(|e| e.confirmed)
    }

    /// 確認済み登録のTOTPコードを検証し、成功ならそのステップを消費する
    /// (再利用防止)。永続化もここで行う。
    fn consume_totp(&self, owner: &str, code: &str, now: DateTime<Utc>) -> bool {
        let mut enrollments = self.enrollments.write().unwrap();
        let Some(e) = enrollments.get_mut(owner).filter(|e| e.confirmed) else {
            return false;
        };
        let Some(step) = fresh_totp_step(e, code, now) else {
            return false;
        };
        e.last_used_step = Some(step);
        self.persist(&enrollments);
        true
    }

    fn is_locked(&self, owner: &str, now: DateTime<Utc>) -> bool {
        self.failures
            .lock()
            .unwrap()
            .get(owner)
            .is_some_and(|f| f.locked_until.is_some_and(|until| until > now))
    }

    /// 失敗を1回記録する。`MAX_FAILED_ATTEMPTS`回目で`LOCKOUT_MINUTES`分
    /// ロックする。窓(ロック時間と同じ長さ)を過ぎた古い失敗は数えない。
    fn record_failure(&self, owner: &str, now: DateTime<Utc>) {
        let mut map = self.failures.lock().unwrap();
        if map.len() >= MAX_TRACKED_KEYS {
            map.retain(|_, f| f.locked_until.is_some_and(|u| u > now) || now - f.window_start < chrono::Duration::minutes(LOCKOUT_MINUTES));
            if map.len() >= MAX_TRACKED_KEYS {
                map.clear();
            }
        }
        let entry = map.entry(owner.to_string()).or_insert(FailureState { failures: 0, window_start: now, locked_until: None });
        if now - entry.window_start >= chrono::Duration::minutes(LOCKOUT_MINUTES) && entry.locked_until.is_none_or(|u| u <= now) {
            *entry = FailureState { failures: 0, window_start: now, locked_until: None };
        }
        entry.failures += 1;
        if entry.failures >= MAX_FAILED_ATTEMPTS {
            entry.locked_until = Some(now + chrono::Duration::minutes(LOCKOUT_MINUTES));
            entry.failures = 0;
            entry.window_start = now;
        }
    }

    fn clear_failures(&self, owner: &str) {
        self.failures.lock().unwrap().remove(owner);
    }

    /// 異常検知(`KeyDecision::Suspended`)時の追加確認。成功すれば
    /// `OVERRIDE_VALIDITY_MINUTES`分だけ`Suspended`状態を上書きする。
    /// 連続失敗は`verify_login`と共有のカウンタでロックされる。
    pub fn verify_for_override(&self, owner: &str, code: &str, now: DateTime<Utc>) -> LoginCheck {
        if self.is_locked(owner, now) {
            return LoginCheck::Locked;
        }
        if self.consume_totp(owner, code, now) {
            self.clear_failures(owner);
            self.overrides.write().unwrap().insert(owner.to_string(), now + chrono::Duration::minutes(OVERRIDE_VALIDITY_MINUTES));
            LoginCheck::Success
        } else {
            self.record_failure(owner, now);
            LoginCheck::Invalid
        }
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

    /// `POST /admin/login/request-otp`のレート制限。owner+接続元IPごとに
    /// `OTP_REQUEST_WINDOW_SECS`秒あたり`OTP_REQUEST_LIMIT`回まで。
    /// 許可する場合は今回の要求を記録して`true`。登録済みか否かに関わらず
    /// 全ownerに同じ制限を課す(存在有無が応答から漏れないように)。
    pub fn allow_otp_request(&self, owner: &str, ip: &str, now: DateTime<Utc>) -> bool {
        let window = chrono::Duration::seconds(OTP_REQUEST_WINDOW_SECS);
        let mut map = self.otp_requests.lock().unwrap();
        if map.len() >= MAX_TRACKED_KEYS {
            map.retain(|_, v| v.iter().any(|t| now - *t < window));
            if map.len() >= MAX_TRACKED_KEYS {
                map.clear();
            }
        }
        let times = map.entry(format!("{owner}\n{ip}")).or_default();
        times.retain(|t| now - *t < window);
        if times.len() >= OTP_REQUEST_LIMIT {
            return false;
        }
        times.push(now);
        true
    }

    /// 有効期限内のメールOTPチャレンジが既にあるか(あれば再発行しない)。
    pub fn has_active_email_challenge(&self, owner: &str, now: DateTime<Utc>) -> bool {
        self.email_challenges.read().unwrap().get(owner).is_some_and(|c| c.expires_at > now)
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

    /// **ログインフロー本体(ユーザー指示「常に二段階認証(メールOTP +
    /// QR/TOTP両方必須)」)**: メールOTPとTOTPコードの**両方**が正しい
    /// 場合のみ成功する。
    ///
    /// - 失敗するとそのownerのメールOTPチャレンジを無効化する(同じOTPで
    ///   総当たりを続けられない)。
    /// - ownerごとに連続`MAX_FAILED_ATTEMPTS`回失敗で`LOCKOUT_MINUTES`分
    ///   ロックし、ロック中は正しいコードでも拒否する。
    /// - 成功時はメールOTPを消費し、TOTPのステップも消費する(再利用不可)。
    ///
    /// 認証不要エンドポイントなので、ロックによる第三者のログイン妨害
    /// (DoS)の余地は残る。復旧はロック時間経過、またはbreak-glassの
    /// `POST /admin/2fa/reset`で行う。
    pub fn verify_login(&self, owner: &str, email_code: &str, totp_code: &str, now: DateTime<Utc>) -> LoginCheck {
        if self.is_locked(owner, now) {
            return LoginCheck::Locked;
        }
        if self.email_otp_matches(owner, email_code, now) && self.consume_totp(owner, totp_code, now) {
            self.email_challenges.write().unwrap().remove(owner);
            self.clear_failures(owner);
            return LoginCheck::Success;
        }
        self.email_challenges.write().unwrap().remove(owner);
        self.record_failure(owner, now);
        LoginCheck::Invalid
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

    fn secret_of(store: &TwoFactorStore, owner: &str) -> Vec<u8> {
        store.enrollments.read().unwrap().get(owner).unwrap().secret.clone()
    }

    fn code_at(store: &TwoFactorStore, owner: &str, t: DateTime<Utc>) -> String {
        format!("{:06}", totp_at(&secret_of(store, owner), t))
    }

    /// 確認済みのownerを作る(確認に使ったステップは`t0`)。
    fn confirmed_store(owner: &str, email: Option<&str>, t0: DateTime<Utc>) -> TwoFactorStore {
        let store = TwoFactorStore::new();
        store.enroll(owner, email.map(String::from), None).unwrap();
        let c = code_at(&store, owner, t0);
        assert!(store.confirm(owner, &c, t0));
        store
    }

    fn step(secs: i64) -> chrono::Duration {
        chrono::Duration::seconds(secs)
    }

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
        let resp = store.enroll("alice", None, None).unwrap();
        assert!(resp.otpauth_url.contains("alice"));
        assert!(resp.qr_svg.is_some(), "QR SVG should be generated");

        let real_code = code_at(&store, "alice", now);
        assert!(!store.has_confirmed_enrollment("alice"));
        assert!(store.confirm("alice", &real_code, now));
        assert!(store.has_confirmed_enrollment("alice"));
    }

    #[test]
    fn confirm_with_wrong_code_fails_and_does_not_confirm() {
        let store = TwoFactorStore::new();
        store.enroll("bob", None, None).unwrap();
        assert!(!store.confirm("bob", "000000", Utc::now()));
        assert!(!store.has_confirmed_enrollment("bob"));
    }

    #[test]
    fn verify_for_override_requires_prior_confirmation() {
        let store = TwoFactorStore::new();
        let now = Utc::now();
        store.enroll("carol", None, None).unwrap();
        let real_code = code_at(&store, "carol", now);

        // まだconfirmしていないので、正しいコードでもoverrideは発行されない。
        assert!(!store.verify_for_override("carol", &real_code, now).is_success());
        assert!(!store.has_valid_override("carol", now));

        assert!(store.confirm("carol", &real_code, now));
        // confirmで使ったステップは再利用できないので、次のステップのコードを使う。
        let later = now + step(60);
        let next_code = code_at(&store, "carol", later);
        assert!(store.verify_for_override("carol", &next_code, later).is_success());
        assert!(store.has_valid_override("carol", later));
    }

    #[test]
    fn override_expires_after_validity_window() {
        let t0 = Utc::now();
        let store = confirmed_store("dave", None, t0);
        let later = t0 + step(60);
        let c = code_at(&store, "dave", later);
        assert!(store.verify_for_override("dave", &c, later).is_success());

        assert!(store.has_valid_override("dave", later + chrono::Duration::minutes(10)));
        assert!(!store.has_valid_override("dave", later + chrono::Duration::minutes(16)));
    }

    #[test]
    fn totp_tolerates_one_step_of_clock_skew_but_not_two() {
        let secret = b"a-fixed-test-secret-".to_vec();
        let now = Utc::now();
        let code_one_step_ago = format!("{:06}", totp_at(&secret, now - step(TOTP_STEP_SECS)));
        let code_two_steps_ago = format!("{:06}", totp_at(&secret, now - step(2 * TOTP_STEP_SECS)));
        assert!(totp_matching_step(&secret, &code_one_step_ago, now).is_some());
        assert!(totp_matching_step(&secret, &code_two_steps_ago, now).is_none());
    }

    /// `verify_login`は**両方**正しくなければ成功しない。片方だけ正しい
    /// 試行は失敗し、さらに失敗するとメールOTPチャレンジが無効化される
    /// (総当たりを同じOTPで続けられない)。
    #[test]
    fn verify_login_requires_both_email_otp_and_totp_to_succeed() {
        let t0 = Utc::now();
        let store = confirmed_store("erin", Some("erin@example.test"), t0);
        let now = t0 + step(60);
        let real_totp = code_at(&store, "erin", now);

        // TOTPのみ正しい → 失敗(チャレンジも無効化される)。
        let otp = store.issue_email_otp("erin", now);
        assert_eq!(store.verify_login("erin", "000000", &real_totp, now), LoginCheck::Invalid);
        assert!(!store.has_active_email_challenge("erin", now), "failure must invalidate the challenge");
        // 無効化後は正しいメールOTPでも通らない。
        assert_eq!(store.verify_login("erin", &otp, &real_totp, now), LoginCheck::Invalid);

        // メールOTPのみ正しい → 失敗。
        let otp = store.issue_email_otp("erin", now);
        assert_eq!(store.verify_login("erin", &otp, "000000", now), LoginCheck::Invalid);

        // 両方正しい → 成功。成功後は同じOTPでの再試行は失敗する。
        let otp = store.issue_email_otp("erin", now);
        assert_eq!(store.verify_login("erin", &otp, &real_totp, now), LoginCheck::Success);
        assert!(!store.verify_login("erin", &otp, &real_totp, now).is_success());
    }

    #[test]
    fn verify_login_fails_without_confirmed_totp_enrollment() {
        let store = TwoFactorStore::new();
        let now = Utc::now();
        store.enroll("frank", Some("frank@example.test".to_string()), None).unwrap();
        let email_otp = store.issue_email_otp("frank", now);
        let real_totp = code_at(&store, "frank", now);
        assert!(!store.verify_login("frank", &email_otp, &real_totp, now).is_success(), "unconfirmed TOTP enrollment must not authorize login");
    }

    #[test]
    fn totp_code_cannot_be_reused_after_login() {
        let t0 = Utc::now();
        let store = confirmed_store("gina", Some("gina@example.test"), t0);
        let now = t0 + step(60);
        let totp = code_at(&store, "gina", now);

        let otp = store.issue_email_otp("gina", now);
        assert!(store.verify_login("gina", &otp, &totp, now).is_success());

        // 同じTOTPコード(盗み見・再送)+新しいメールOTP → 拒否。
        let otp2 = store.issue_email_otp("gina", now);
        assert!(!store.verify_login("gina", &otp2, &totp, now).is_success());
        // 同じ窓内でも、次のステップの新しいコードなら通る。
        let later = now + step(TOTP_STEP_SECS);
        let totp2 = code_at(&store, "gina", later);
        let otp3 = store.issue_email_otp("gina", later);
        assert!(store.verify_login("gina", &otp3, &totp2, later).is_success());
    }

    #[test]
    fn totp_code_used_for_confirm_cannot_be_replayed_for_override() {
        let t0 = Utc::now();
        let store = TwoFactorStore::new();
        store.enroll("hal", None, None).unwrap();
        let c = code_at(&store, "hal", t0);
        assert!(store.confirm("hal", &c, t0));
        assert!(!store.verify_for_override("hal", &c, t0).is_success());
    }

    #[test]
    fn enroll_is_refused_for_confirmed_owner_until_reset() {
        let t0 = Utc::now();
        let store = confirmed_store("ivy", None, t0);
        let before = secret_of(&store, "ivy");
        assert!(matches!(store.enroll("ivy", None, None), Err(EnrollError::AlreadyConfirmed)));
        assert_eq!(secret_of(&store, "ivy"), before, "secret must not be overwritten");
        assert!(store.has_confirmed_enrollment("ivy"));

        assert!(store.reset("ivy"));
        assert!(!store.has_confirmed_enrollment("ivy"));
        assert!(store.enroll("ivy", None, None).is_ok());
        assert!(!store.reset("nobody"));
    }

    #[test]
    fn unconfirmed_enrollment_can_be_overwritten() {
        let store = TwoFactorStore::new();
        store.enroll("jon", None, None).unwrap();
        let first = secret_of(&store, "jon");
        store.enroll("jon", None, None).unwrap();
        assert_ne!(first, secret_of(&store, "jon"));
    }

    #[test]
    fn five_consecutive_failures_lock_the_owner_even_for_correct_codes() {
        let t0 = Utc::now();
        let store = confirmed_store("kim", Some("kim@example.test"), t0);
        let now = t0 + step(60);

        for _ in 0..MAX_FAILED_ATTEMPTS {
            assert_eq!(store.verify_login("kim", "000000", "000000", now), LoginCheck::Invalid);
        }
        // ロック中は正しい2要素でも拒否。
        let otp = store.issue_email_otp("kim", now);
        let totp = code_at(&store, "kim", now);
        assert_eq!(store.verify_login("kim", &otp, &totp, now), LoginCheck::Locked);
        assert_eq!(store.verify_for_override("kim", &totp, now), LoginCheck::Locked);

        // ロック時間経過後は復帰する。
        let after = now + chrono::Duration::minutes(LOCKOUT_MINUTES) + step(1);
        let otp = store.issue_email_otp("kim", after);
        let totp = code_at(&store, "kim", after);
        assert_eq!(store.verify_login("kim", &otp, &totp, after), LoginCheck::Success);
    }

    #[test]
    fn failures_are_per_owner_and_success_resets_the_counter() {
        let t0 = Utc::now();
        let store = confirmed_store("lee", Some("lee@example.test"), t0);
        let now = t0 + step(60);
        for _ in 0..(MAX_FAILED_ATTEMPTS - 1) {
            assert_eq!(store.verify_login("lee", "000000", "000000", now), LoginCheck::Invalid);
        }
        // 他ownerの失敗はleeに影響しない。
        for _ in 0..10 {
            store.verify_login("someone-else", "000000", "000000", now);
        }
        let otp = store.issue_email_otp("lee", now);
        let totp = code_at(&store, "lee", now);
        assert_eq!(store.verify_login("lee", &otp, &totp, now), LoginCheck::Success);
        // 成功でカウンタがリセットされ、また4回失敗してもロックされない。
        for _ in 0..(MAX_FAILED_ATTEMPTS - 1) {
            assert_eq!(store.verify_login("lee", "000000", "000000", now), LoginCheck::Invalid);
        }
    }

    #[test]
    fn override_failures_also_lock() {
        let t0 = Utc::now();
        let store = confirmed_store("max", None, t0);
        let now = t0 + step(60);
        for _ in 0..MAX_FAILED_ATTEMPTS {
            assert_eq!(store.verify_for_override("max", "000000", now), LoginCheck::Invalid);
        }
        let totp = code_at(&store, "max", now);
        assert_eq!(store.verify_for_override("max", &totp, now), LoginCheck::Locked);
    }

    #[test]
    fn otp_requests_are_rate_limited_per_owner_and_ip() {
        let store = TwoFactorStore::new();
        let now = Utc::now();
        for _ in 0..OTP_REQUEST_LIMIT {
            assert!(store.allow_otp_request("nia", "203.0.113.1", now));
        }
        assert!(!store.allow_otp_request("nia", "203.0.113.1", now));
        // 別IP・別ownerは独立。
        assert!(store.allow_otp_request("nia", "203.0.113.2", now));
        assert!(store.allow_otp_request("other", "203.0.113.1", now));
        // 窓が過ぎれば再び許可。
        let later = now + step(OTP_REQUEST_WINDOW_SECS + 1);
        assert!(store.allow_otp_request("nia", "203.0.113.1", later));
    }

    #[test]
    fn active_email_challenge_is_detected_until_it_expires() {
        let store = TwoFactorStore::new();
        let now = Utc::now();
        assert!(!store.has_active_email_challenge("oz", now));
        store.issue_email_otp("oz", now);
        assert!(store.has_active_email_challenge("oz", now));
        assert!(!store.has_active_email_challenge("oz", now + chrono::Duration::minutes(EMAIL_OTP_VALIDITY_MINUTES + 1)));
    }

    /// プロセス再起動を想定し、確認済みTOTP登録(と消費済みステップ)が
    /// 独立した第二の`TwoFactorStore`インスタンスへ実ファイル経由で
    /// 引き継がれ、権限が所有者のみになっていることを検証する。
    #[test]
    fn confirmed_enrollment_survives_reload_into_a_fresh_instance_via_disk() {
        let tmp_dir = std::env::temp_dir().join(format!("owsrv-2fa-test-{}", uuid::Uuid::new_v4()));
        let path = tmp_dir.join("sub").join("2fa-store.json");
        let now = Utc::now();
        // 環境変数を介さずに永続化パスを直接指定する(並列テストと衝突しない)。
        let store1 = TwoFactorStore { persistence_path: Some(path.clone()), ..TwoFactorStore::default() };
        store1.enroll("carol", None, None).unwrap();
        let real_code = code_at(&store1, "carol", now);
        assert!(store1.confirm("carol", &real_code, now));

        let loaded = load_enrollments_from_path(&path);
        let e = loaded.get("carol").expect("persisted");
        assert!(e.confirmed);
        assert!(e.last_used_step.is_some(), "consumed step must be persisted");
        let store2 = TwoFactorStore { enrollments: RwLock::new(loaded), ..TwoFactorStore::default() };
        assert!(store2.has_confirmed_enrollment("carol"), "confirmed enrollment must survive reload from disk");
        // 再起動後も、消費済みステップのコードは再利用できない。
        assert!(!store2.verify_for_override("carol", &real_code, now).is_success());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(file_mode, 0o600, "store file must be owner-only");
            let dir_mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700, "newly created store dir must be 0700");
        }
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn old_store_files_without_last_used_step_still_load() {
        let tmp_dir = std::env::temp_dir().join(format!("owsrv-2fa-old-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let path = tmp_dir.join("old.json");
        std::fs::write(&path, r#"{"zed":{"secret":[1,2,3],"confirmed":true,"email":null,"phone_number":null}}"#).unwrap();
        let loaded = load_enrollments_from_path(&path);
        assert!(loaded.get("zed").is_some_and(|e| e.confirmed && e.last_used_step.is_none()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn missing_persistence_file_starts_empty_without_crashing() {
        let tmp_dir = std::env::temp_dir().join(format!("owsrv-2fa-missing-{}", uuid::Uuid::new_v4()));
        let loaded = load_enrollments_from_path(&tmp_dir.join("does-not-exist.json"));
        assert!(loaded.is_empty());
    }
}
