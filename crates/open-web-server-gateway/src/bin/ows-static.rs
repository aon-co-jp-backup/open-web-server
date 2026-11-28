//! ows-static: 紹介ページ用の小さな静的ファイル配信(標準ライブラリのみ)。
//! VPS で `python3 -m http.server <port> --bind 127.0.0.1` の代わりに使う(前面は open-web-server が受ける)。
//!
//! 使い方: `ows-static [--no-dir-redirect] <port> [dir]` (dir 省略時はカレントディレクトリ。待ち受けは 127.0.0.1 のみ)
//! `--no-dir-redirect`: 末尾 `/` の無いディレクトリも 301 せず index.html を返す(strip_prefix 経由で接頭辞が消えるのを防ぐ)。
//! - GET / HEAD のみ(それ以外は 405)。ディレクトリは `index.html` を返し、無ければ 404(一覧は出さない)。
//! - `..` や絶対パスでルート外へ出る要求は 404。末尾 `/` の無いディレクトリは `/` 付きへ 301(http.server と同じ)。
//!
//! ows-static: a tiny std-only static file server replacing `python3 -m http.server` for intro pages
//! (bound to 127.0.0.1; open-web-server is the front). GET/HEAD only, `index.html` for directories,
//! no directory listings, no escaping the root.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("txt") | Some("md") | Some("sha256") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/vnd.microsoft.icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("pdf") => "application/pdf",
        Some("mp4") => "video/mp4",
        Some("mp3") => "audio/mpeg",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// 要求パスをルート内の実パスへ。ルート外へ出るなら None。
fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let raw = url_path.split(['?', '#']).next().unwrap_or("");
    let decoded = percent_decode(raw)?;
    let mut p = root.to_path_buf();
    for c in Path::new(decoded.trim_start_matches('/')).components() {
        match c {
            Component::Normal(x) => p.push(x),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(p)
}

fn respond(s: &mut TcpStream, code: &str, headers: &[(&str, String)], body: &[u8], head: bool) {
    let mut h = format!("HTTP/1.1 {code}\r\nServer: ows-static\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    h.push_str("\r\n");
    let _ = s.write_all(h.as_bytes());
    if !head {
        let _ = s.write_all(body);
    }
}

fn handle(mut s: TcpStream, root: &Path, no_redirect: bool) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(15)));
    let mut r = BufReader::new(match s.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    });
    let mut line = String::new();
    if r.by_ref().take(8192).read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    // ヘッダーは読み捨てる(上限つき)。
    let mut hl = String::new();
    for _ in 0..100 {
        hl.clear();
        if r.by_ref().take(8192).read_line(&mut hl).unwrap_or(0) == 0 || hl == "\r\n" || hl == "\n" {
            break;
        }
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let head = method == "HEAD";
    if method != "GET" && !head {
        respond(&mut s, "405 Method Not Allowed", &[("Allow", "GET, HEAD".into())], b"", false);
        return;
    }
    let not_found = |s: &mut TcpStream| {
        respond(s, "404 Not Found", &[("Content-Type", "text/plain; charset=utf-8".into())], b"404 Not Found\n", head)
    };
    let Some(mut path) = resolve(root, target) else { return not_found(&mut s) };
    if path.is_dir() {
        let raw = target.split(['?', '#']).next().unwrap_or("");
        if !raw.ends_with('/') && !no_redirect {
            let q = &target[raw.len()..];
            respond(&mut s, "301 Moved Permanently", &[("Location", format!("{raw}/{q}"))], b"", head);
            return;
        }
        path.push("index.html");
    }
    match fs::read(&path) {
        Ok(body) if path.is_file() => {
            respond(&mut s, "200 OK", &[("Content-Type", mime(&path).into())], &body, head)
        }
        _ => not_found(&mut s),
    }
}

fn main() {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let no_redirect = all.iter().any(|a| a == "--no-dir-redirect");
    let mut args = all.into_iter().filter(|a| a != "--no-dir-redirect");
    let port: u16 = match args.next().and_then(|p| p.parse().ok()) {
        Some(p) => p,
        None => {
            eprintln!("usage: ows-static <port> [dir]");
            std::process::exit(2);
        }
    };
    let root = fs::canonicalize(args.next().unwrap_or_else(|| ".".into())).expect("dir not found");
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind failed");
    eprintln!("ows-static: 127.0.0.1:{port} -> {}", root.display());
    for conn in listener.incoming().flatten() {
        let root = root.clone();
        std::thread::spawn(move || handle(conn, &root, no_redirect));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_stays_in_root() {
        let r = Path::new("/srv");
        assert_eq!(resolve(r, "/a/b.html?x=1").unwrap(), PathBuf::from("/srv/a/b.html"));
        assert_eq!(resolve(r, "/%E6%97%A5.html").unwrap(), PathBuf::from("/srv/日.html"));
        assert!(resolve(r, "/../etc/passwd").is_none());
        assert!(resolve(r, "/a/%2e%2e/%2e%2e/x").is_none());
        assert!(resolve(r, "/%zz").is_none());
    }

    #[test]
    fn mime_types() {
        assert_eq!(mime(Path::new("a.HTML")), "text/html; charset=utf-8");
        assert_eq!(mime(Path::new("a.exe")), "application/octet-stream");
    }
}
