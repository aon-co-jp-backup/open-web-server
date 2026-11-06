//! 実バイナリ(`open-web-server`)を子プロセスとして起動し、外から実プロトコルで
//! SFTP と 2FA ログイン(メールOTP+TOTP)を通しで検証する結合テスト。
//! `cargo test -p open-web-server-gateway --features "sftp admin-2fa" --test e2e_real_binary`
#![cfg(all(feature = "sftp", feature = "admin-2fa"))]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hmac::{Hmac, KeyInit, Mac};
use russh::keys::{Algorithm, PrivateKey, PrivateKeyWithHashAlg};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_port(port: u16) {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("port {port} did not open");
}

fn spawn_server(dir: &std::path::Path, envs: &[(&str, String)]) -> Proc {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_open-web-server"));
    let log = std::fs::File::create(dir.join(format!("server-{}.log", free_port()))).unwrap();
    cmd.current_dir(dir).stdout(log.try_clone().unwrap()).stderr(log);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    Proc(cmd.spawn().expect("spawn open-web-server"))
}

// ---------------- 最小のSMTPサーバー(平文。届いた本文を保存する) ----------------

fn spawn_smtp_sink() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let mails = Arc::new(Mutex::new(Vec::new()));
    let m = mails.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let m = m.clone();
            std::thread::spawn(move || {
                let mut w = stream.try_clone().unwrap();
                let mut r = BufReader::new(stream);
                let _ = w.write_all(b"220 sink ESMTP\r\n");
                let mut line = String::new();
                let mut data = String::new();
                let mut in_data = false;
                let mut login_step = 0;
                loop {
                    line.clear();
                    if r.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if in_data {
                        if line == ".\r\n" {
                            in_data = false;
                            m.lock().unwrap().push(std::mem::take(&mut data));
                            let _ = w.write_all(b"250 queued\r\n");
                        } else {
                            data.push_str(&line);
                        }
                        continue;
                    }
                    let up = line.to_ascii_uppercase();
                    let reply: &[u8] = if login_step == 1 {
                        login_step = 2;
                        b"334 UGFzc3dvcmQ6\r\n"
                    } else if login_step == 2 {
                        login_step = 0;
                        b"235 ok\r\n"
                    } else if up.starts_with("EHLO") {
                        b"250-sink\r\n250 AUTH PLAIN LOGIN\r\n"
                    } else if up.starts_with("AUTH PLAIN") {
                        b"235 ok\r\n"
                    } else if up.starts_with("AUTH LOGIN") {
                        login_step = 1;
                        b"334 VXNlcm5hbWU6\r\n"
                    } else if up.starts_with("DATA") {
                        in_data = true;
                        b"354 go\r\n"
                    } else if up.starts_with("QUIT") {
                        let _ = w.write_all(b"221 bye\r\n");
                        break;
                    } else {
                        b"250 ok\r\n"
                    };
                    let _ = w.write_all(reply);
                }
            });
        }
    });
    (port, mails)
}

// ---------------- TOTP(RFC 6238、SHA-1、6桁、30秒) ----------------

fn base32_decode(s: &str) -> Vec<u8> {
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in s.trim_end_matches('=').chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            '2'..='7' => c as u32 - '2' as u32 + 26,
            _ => continue,
        };
        bits = (bits << 5) | v;
        n += 5;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
            bits &= (1 << n) - 1;
        }
    }
    out
}

/// TOTP は使い回し(同じ30秒ステップの再利用)を拒否する設計(admin-2fa-hardening)なので、
/// 成功で消費した後に再度成功させるテストは、次のステップまで待つ。
fn wait_next_totp_step() {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    std::thread::sleep(Duration::from_secs(30 - now % 30 + 1));
}

fn totp_now(secret_b32: &str) -> String {
    let secret = base32_decode(secret_b32);
    let step = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() / 30;
    let mut mac = <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(&secret).unwrap();
    mac.update(&step.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let off = (h[19] & 0xf) as usize;
    let bin = ((h[off] as u32 & 0x7f) << 24) | ((h[off + 1] as u32) << 16) | ((h[off + 2] as u32) << 8) | h[off + 3] as u32;
    format!("{:06}", bin % 1_000_000)
}

fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    if !body.is_empty() {
        req.push_str("Content-Type: application/json\r\n");
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    let _ = s.read_to_string(&mut resp);
    let status = resp.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

/// メール本文を取り出す(base64 / quoted-printable / 素のテキストに対応)。
fn decode_mail_body(raw: &str) -> String {
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or(("", raw));
    let head = head.to_ascii_lowercase();
    if head.contains("base64") {
        let mut bits = 0u32;
        let mut n = 0;
        let mut out = Vec::new();
        for c in body.chars() {
            let v = match c {
                'A'..='Z' => c as u32 - 65,
                'a'..='z' => c as u32 - 97 + 26,
                '0'..='9' => c as u32 - 48 + 52,
                '+' => 62,
                '/' => 63,
                _ => continue,
            };
            bits = (bits << 6) | v;
            n += 6;
            if n >= 8 {
                n -= 8;
                out.push((bits >> n) as u8);
                bits &= (1 << n) - 1;
            }
        }
        return String::from_utf8_lossy(&out).into_owned();
    }
    body.to_string()
}

fn json_field(body: &str, key: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_else(|_| panic!("not json: {body}"));
    v[key].as_str().unwrap_or_else(|| panic!("no field {key} in {body}")).to_string()
}

#[test]
fn two_factor_login_end_to_end_over_real_http_and_smtp() {
    let dir = std::env::temp_dir().join(format!("ows-e2e-2fa-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (smtp_port, mails) = spawn_smtp_sink();
    let port = free_port();
    let admin = "e2e-admin-token-not-secret";
    let srv = spawn_server(
        &dir,
        &[
            ("OPEN_WEB_SERVER_BIND", format!("127.0.0.1:{port}")),
            ("OPEN_WEB_SERVER_ADMIN_TOKEN", admin.into()),
            ("OPEN_WEB_SERVER_2FA_STORE_PATH", dir.join("2fa.json").display().to_string()),
            ("OPEN_WEB_SERVER_SMTP_HOST", "127.0.0.1".into()),
            ("OPEN_WEB_SERVER_SMTP_PORT", smtp_port.to_string()),
            ("OPEN_WEB_SERVER_SMTP_SECURITY", "none".into()),
            ("OPEN_WEB_SERVER_SMTP_USERNAME", "u".into()),
            ("OPEN_WEB_SERVER_SMTP_PASSWORD", "p".into()),
            ("OPEN_WEB_SERVER_SMTP_FROM", "noreply@example.test".into()),
        ],
    );
    wait_port(port);
    let auth = [("x-admin-token", admin)];

    // 登録: 管理者トークン無しは拒否
    let (st, _) = http(port, "POST", "/open_web_server.admin.v1.Admin/Enroll2fa", &[], r#"{"owner":"alice","email":"alice@example.test"}"#);
    assert!(st == 401 || st == 403, "enroll without token must be rejected, got {st}");
    let (st, body) = http(port, "POST", "/open_web_server.admin.v1.Admin/Enroll2fa", &auth, r#"{"owner":"alice","email":"alice@example.test"}"#);
    assert!(st == 200 || st == 201, "{body}");
    let secret = json_field(&body, "secret_base32");

    // 間違ったTOTPでは確認できない / 正しいコードで確認できる
    let (st, _) = http(port, "POST", "/open_web_server.admin.v1.Admin/Confirm2fa", &auth, r#"{"owner":"alice","code":"abcdef"}"#);
    assert_eq!(st, 400);
    let (st, body) = http(port, "POST", "/open_web_server.admin.v1.Admin/Confirm2fa", &auth, &format!(r#"{{"owner":"alice","code":"{}"}}"#, totp_now(&secret)));
    assert_eq!(st, 200, "{body}");

    // メールOTPを請求 → 実SMTPで届いた本文からコードを取り出す
    let (st, _) = http(port, "POST", "/open_web_server.admin.v1.Admin/RequestLoginOtp", &[], r#"{"owner":"alice"}"#);
    assert_eq!(st, 200);
    let end = Instant::now() + Duration::from_secs(10);
    let otp = loop {
        if let Some(m) = mails.lock().unwrap().last() {
            let text = decode_mail_body(m);
            let digits: Vec<&str> = text.split(|c: char| !c.is_ascii_digit()).filter(|s| s.len() == 6).collect();
            if let Some(c) = digits.first() {
                break c.to_string();
            }
        }
        if Instant::now() >= end {
            let logs: String = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("server-")).map(|e| std::fs::read_to_string(e.path()).unwrap_or_default()).collect();
            let tail: Vec<&str> = logs.lines().rev().take(15).collect();
            let got = mails.lock().unwrap().clone();
            panic!("OTP mail not parsed; mails: {got:?}; server log tail: {tail:#?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    // 片方が誤りなら拒否される
    let (st, _) = http(port, "POST", "/open_web_server.admin.v1.Admin/VerifyLogin", &[], &format!(r#"{{"owner":"alice","email_otp":"{otp}","totp_code":"abcdef"}}"#));
    assert_eq!(st, 401, "wrong TOTP must be rejected");
    let (st, _) = http(port, "POST", "/open_web_server.admin.v1.Admin/VerifyLogin", &[], &format!(r#"{{"owner":"alice","email_otp":"xxxxxx","totp_code":"{}"}}"#, totp_now(&secret)));
    assert_eq!(st, 401, "wrong email OTP must be rejected");

    // 失敗した検証は、そのメールOTPを失効させる(総当たり対策、admin-2fa-hardening)。
    // よって成功ケースの前にメールOTPを請求し直し、新しく届いたものを使う。
    let before = mails.lock().unwrap().len();
    let (st, _) = http(port, "POST", "/open_web_server.admin.v1.Admin/RequestLoginOtp", &[], r#"{"owner":"alice"}"#);
    assert_eq!(st, 200);
    let end = Instant::now() + Duration::from_secs(10);
    let otp = loop {
        {
            let g = mails.lock().unwrap();
            if g.len() > before {
                let text = decode_mail_body(g.last().unwrap());
                if let Some(c) = text.split(|c: char| !c.is_ascii_digit()).filter(|s| s.len() == 6).next() {
                    break c.to_string();
                }
            }
        }
        assert!(Instant::now() < end, "second OTP mail did not arrive");
        std::thread::sleep(Duration::from_millis(100));
    };
    wait_next_totp_step(); // 確認(Confirm2fa)で消費したステップの再利用は拒否されるため
    // 両方正しければBearerキーが発行され、それで管理APIに入れる
    let (st, body) = http(port, "POST", "/open_web_server.admin.v1.Admin/VerifyLogin", &[], &format!(r#"{{"owner":"alice","email_otp":"{otp}","totp_code":"{}"}}"#, totp_now(&secret)));
    assert_eq!(st, 200, "{body}");
    let key = json_field(&body, "key");
    let bearer = format!("Bearer {key}");
    let (st, _) = http(port, "POST", "/open_web_server.admin.v1.Admin/ListTenants", &[("authorization", &bearer)], "{}");
    assert_eq!(st, 200, "issued key must open the admin API");
    let (st, _) = http(port, "POST", "/open_web_server.admin.v1.Admin/ListTenants", &[], "{}");
    assert!(st == 401 || st == 403, "no credential must be rejected, got {st}");

    // メールOTPは使い捨て
    wait_next_totp_step(); // TOTP が新しいステップでも、メールOTPの使い捨てで拒否されることを確かめる
    let (st, _) = http(port, "POST", "/open_web_server.admin.v1.Admin/VerifyLogin", &[], &format!(r#"{{"owner":"alice","email_otp":"{otp}","totp_code":"{}"}}"#, totp_now(&secret)));
    assert_eq!(st, 401, "email OTP must be single-use");

    // 再起動しても登録が残る(ディスク永続化)
    drop(srv);
    let port2 = free_port();
    let _srv2 = spawn_server(
        &dir,
        &[
            ("OPEN_WEB_SERVER_BIND", format!("127.0.0.1:{port2}")),
            ("OPEN_WEB_SERVER_ADMIN_TOKEN", admin.into()),
            ("OPEN_WEB_SERVER_2FA_STORE_PATH", dir.join("2fa.json").display().to_string()),
        ],
    );
    wait_port(port2);
    wait_next_totp_step();
    let (st, body) = http(port2, "POST", "/open_web_server.admin.v1.Admin/Confirm2fa", &auth, &format!(r#"{{"owner":"alice","code":"{}"}}"#, totp_now(&secret)));
    assert_eq!(st, 200, "enrollment must survive restart: {body}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------- SFTP ----------------

struct AcceptAny;
impl russh::client::Handler for AcceptAny {
    type Error = russh::Error;
    fn check_server_key(&mut self, _k: &russh::keys::PublicKeyOrCertificate) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(true) }
    }
}

struct HostKeyCapture(Arc<Mutex<Option<String>>>);
impl russh::client::Handler for HostKeyCapture {
    type Error = russh::Error;
    fn check_server_key(&mut self, k: &russh::keys::PublicKeyOrCertificate) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        *self.0.lock().unwrap() = Some(format!("{k:?}"));
        async { Ok(true) }
    }
}

async fn connect(port: u16) -> russh::client::Handle<AcceptAny> {
    russh::client::connect(Arc::new(russh::client::Config::default()), ("127.0.0.1", port), AcceptAny).await.unwrap()
}

async fn host_key_of(port: u16) -> String {
    let cap = Arc::new(Mutex::new(None));
    let _h = russh::client::connect(Arc::new(russh::client::Config::default()), ("127.0.0.1", port), HostKeyCapture(cap.clone())).await.unwrap();
    let v = cap.lock().unwrap().clone();
    v.unwrap()
}

#[tokio::test]
async fn sftp_end_to_end_against_real_binary() {
    let dir = std::env::temp_dir().join(format!("ows-e2e-sftp-{}", std::process::id()));
    let root = dir.join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(dir.join("secret-outside.txt"), b"must not be readable").unwrap();

    let mut rng = russh::keys::key::safe_rng();
    let good = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
    let bad = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
    let ak = dir.join("authorized_keys");
    std::fs::write(&ak, format!("{}\n", good.public_key().to_openssh().unwrap())).unwrap();

    let http_port = free_port();
    let sftp_port = free_port();
    let mk = |http: u16, sftp: u16| -> Vec<(&'static str, String)> {
        vec![
            ("OPEN_WEB_SERVER_BIND", format!("127.0.0.1:{http}")),
            ("OPEN_WEB_SERVER_SFTP_BIND", format!("127.0.0.1:{sftp}")),
            ("OPEN_WEB_SERVER_SFTP_ROOT", root.display().to_string()),
            ("OPEN_WEB_SERVER_SFTP_AUTHORIZED_KEYS_FILE", ak.display().to_string()),
            ("OPEN_WEB_SERVER_SFTP_HOST_KEY_FILE", dir.join("host_key").display().to_string()),
            ("OPEN_WEB_SERVER_SFTP_ALLOW_PASSWORD_AUTH", "true".into()),
            ("OPEN_WEB_SERVER_SFTP_PASSWORD", "e2e-pass".into()),
        ]
    };
    let srv = spawn_server(&dir, &mk(http_port, sftp_port));
    wait_port(sftp_port);

    // 公開鍵: 登録済みは成功、未登録は拒否
    let mut h = connect(sftp_port).await;
    let ok = h.authenticate_publickey("u", PrivateKeyWithHashAlg::new(Arc::new(good.clone()), None)).await.unwrap();
    assert!(matches!(ok, russh::client::AuthResult::Success));
    let mut h2 = connect(sftp_port).await;
    let ng = h2.authenticate_publickey("u", PrivateKeyWithHashAlg::new(Arc::new(bad), None)).await.unwrap();
    assert!(!matches!(ng, russh::client::AuthResult::Success), "unregistered key must be rejected");

    // パスワード: 正しい/誤り
    let mut h3 = connect(sftp_port).await;
    assert!(!matches!(h3.authenticate_password("u", "wrong").await.unwrap(), russh::client::AuthResult::Success));
    let mut h4 = connect(sftp_port).await;
    assert!(matches!(h4.authenticate_password("u", "e2e-pass").await.unwrap(), russh::client::AuthResult::Success));

    // ファイル操作
    let ch = h.channel_open_session().await.unwrap();
    ch.request_subsystem(true, "sftp").await.unwrap();
    let sftp = russh_sftp::client::SftpSession::new(ch.into_stream()).await.unwrap();
    sftp.create_dir("d").await.unwrap();
    let big: Vec<u8> = (0..1_048_576u32).map(|i| (i % 251) as u8).collect();
    sftp.write("d/big.bin", &big).await.unwrap();
    assert_eq!(sftp.read("d/big.bin").await.unwrap(), big);
    sftp.rename("d/big.bin", "d/big2.bin").await.unwrap();
    assert!(root.join("d/big2.bin").exists());
    // setstat: 更新日時とサイズ(切り詰め)が実ファイルへ反映される
    let mut attrs = sftp.metadata("d/big2.bin").await.unwrap();
    attrs.mtime = Some(1_700_000_000);
    attrs.size = Some(10);
    sftp.set_metadata("d/big2.bin", attrs).await.unwrap();
    let meta = std::fs::metadata(root.join("d/big2.bin")).unwrap();
    assert_eq!(meta.len(), 10, "setstat size must truncate");
    let secs = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert_eq!(secs, 1_700_000_000, "setstat mtime must be applied");
    // ルート外への脱出は拒否
    assert!(sftp.read("../secret-outside.txt").await.is_err(), "traversal must be refused");
    assert!(sftp.write("../escaped.txt", b"x").await.is_err());
    assert!(!dir.join("escaped.txt").exists());
    sftp.remove_file("d/big2.bin").await.unwrap();
    drop(sftp);

    // 再起動してもホスト鍵が変わらない
    let before = host_key_of(sftp_port).await;
    drop(srv);
    let sftp_port2 = free_port();
    let _srv2 = spawn_server(&dir, &mk(free_port(), sftp_port2));
    wait_port(sftp_port2);
    assert_eq!(before, host_key_of(sftp_port2).await, "SFTP host key must persist across restarts");
    let _ = std::fs::remove_dir_all(&dir);
}
