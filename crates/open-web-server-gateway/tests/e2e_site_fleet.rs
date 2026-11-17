//! `sites.toml` の process サイトを、実バイナリ(open-web-server)が `SiteFleet` で
//! 実際に起動・監視し、実HTTPで転送・同時接続上限(503)・crash復帰できることの通し確認。
//! `cargo test -p open-web-server-gateway --features site-fleet --test e2e_site_fleet`
//!
//! 起動する分身には RPoem の `open-runo-dummy-http-server`(`/health`=200、`/crash`=自己終了、
//! 1接続ずつ直列に処理)を使う。環境変数 `OPEN_RUNO_DUMMY_SERVER` で場所を上書きできる。
//! 見つからなければ(兄弟リポジトリ未配置)理由を出して通過させる。
#![cfg(feature = "site-fleet")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

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

fn dummy_server() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OPEN_RUNO_DUMMY_SERVER") {
        return Some(PathBuf::from(p));
    }
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../RPoem/target/debug");
    ["open-runo-dummy-http-server", "open-runo-dummy-http-server.exe"]
        .iter()
        .map(|n| base.join(n))
        .find(|p| p.exists())
}

/// `Host` を指定した素のHTTP/1.1 GET。(status, headers+body 全文) を返す。接続失敗は None。
fn get(port: u16, host: &str, path: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes())
        .ok()?;
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    let status = buf.split_whitespace().nth(1)?.parse().ok()?;
    Some((status, buf))
}

fn wait_until(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn process_site_is_started_forwarded_limited_and_recovers() {
    let Some(dummy) = dummy_server() else {
        eprintln!("SKIP: open-runo-dummy-http-server not found (set OPEN_RUNO_DUMMY_SERVER)");
        return;
    };
    let dir = std::env::temp_dir().join(format!("ows_site_fleet_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let app_port = free_port(); // 上限つきサイト(固定ポート。直接つなぐ必要があるため)
    let sites = format!(
        r#"
[[site]]
id = "limited"
hosts = ["limited.test"]
kind = "process"
port = {app_port}
command = {cmd:?}
health_path = "/health"
limits = {{ max_concurrent = 1 }}

[[site]]
id = "auto"
hosts = ["auto.test"]
kind = "process"
port = "auto"
command = {cmd:?}
health_path = "/health"
"#,
        cmd = dummy.display().to_string()
    );
    let sites_path = dir.join("sites.toml");
    std::fs::write(&sites_path, sites).unwrap();

    let ows_port = free_port();
    let log = std::fs::File::create(dir.join("server.log")).unwrap();
    let _srv = Proc(
        Command::new(env!("CARGO_BIN_EXE_open-web-server"))
            .current_dir(&dir)
            .env("OPEN_WEB_SERVER_BIND", format!("127.0.0.1:{ows_port}"))
            .env("OPEN_WEB_SERVER_ADMIN_TOKEN", "e2e-admin-token-not-secret")
            .env("OPEN_WEB_SERVER_SITES_FILE", &sites_path)
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("spawn open-web-server"),
    );

    // 1) 本体が分身を起動し、Hostで振り分けて転送できる(両サイト)
    for host in ["limited.test", "auto.test"] {
        assert!(
            wait_until(30, || get(ows_port, host, "/health").is_some_and(|(s, _)| s == 200)),
            "{host}: /health が 200 にならない(本体が起動・転送できていない)"
        );
    }

    // 2) 同時接続上限: 分身へ直接つないで1接続目を塞ぎ(直列処理のため以後は待たされる)、
    //    本体経由の1件が枠を持ったまま待つ間に、2件目は即503+Retry-After
    let mut stuck = TcpStream::connect(("127.0.0.1", app_port)).unwrap();
    stuck.write_all(b"GET /health").unwrap(); // 改行を送らない=分身はここで待ち続ける
    std::thread::sleep(Duration::from_millis(300));
    let holder = std::thread::spawn(move || get(ows_port, "limited.test", "/health"));
    std::thread::sleep(Duration::from_millis(700)); // holderが枠を取って待つ時間
    let (st, body) = get(ows_port, "limited.test", "/health").expect("2nd request");
    assert_eq!(st, 503, "上限超過は503: {body}");
    assert!(body.to_ascii_lowercase().contains("retry-after: 1"), "Retry-After が無い: {body}");
    // 他サイトは影響を受けない
    assert_eq!(get(ows_port, "auto.test", "/health").map(|r| r.0), Some(200));
    // 塞ぎを外せば、待っていた1件目は成功し、枠が戻る
    drop(stuck);
    let first = holder.join().unwrap();
    assert_eq!(first.map(|r| r.0), Some(200), "待っていた1件目が成功しない");
    assert!(
        wait_until(10, || get(ows_port, "limited.test", "/health").is_some_and(|(s, _)| s == 200)),
        "枠が戻らない"
    );

    // 3) crash復帰: /crash で分身が自己終了 → SiteFleet が再起動し /health が戻る
    let _ = get(ows_port, "auto.test", "/crash");
    assert!(
        wait_until(30, || get(ows_port, "auto.test", "/health").is_some_and(|(s, _)| s == 200)),
        "crash後に復帰しない"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
