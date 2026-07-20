//! 静的ファイル配信ハンドラ(Apache/Nginxの静的配信相当)。
//!
//! 設定されたdocroot配下のファイルをGETで配信する。ディレクトリトラバーサル
//! 対策として、リクエストパスに `..` を含む場合は即座に拒否し、さらに
//! 解決後の絶対パスを正規化(`canonicalize`)した上でdocroot配下に留まって
//! いることを再確認する(シンボリックリンクでdocroot外へ逃げるケースも
//! この二段目のチェックで検出できる — `canonicalize`はシンボリックリンクを
//! 解決した実体パスを返すため)。

use std::path::{Path, PathBuf};

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

use crate::response::{text_response, BoxBody};

/// リクエストパス(例: `/images/foo.png`)を受け取り、`docroot`配下のファイルを
/// 読み込んで返す。
///
/// 拒否条件(すべて`403 Forbidden`):
/// - パスに `..` セグメントが含まれる
/// - パスが絶対パスのエスケープを意図している(例: 別ドライブ指定等)
/// - 正規化後の絶対パスが `docroot` の正規化後パスの配下でない
///
/// ファイルが存在しない場合は `404 Not Found`。
pub fn serve(docroot: &Path, request_path: &str) -> Response<BoxBody> {
    // クエリ文字列は呼び出し元で既に除去されている前提だが、念のため防御。
    let request_path = request_path.split('?').next().unwrap_or(request_path);

    if request_path.contains("..") {
        return text_response(StatusCode::FORBIDDEN, "path traversal rejected");
    }

    // 先頭の '/' を剥がし、docrootからの相対パスとして結合する。
    let relative = request_path.trim_start_matches('/');
    let relative_path = Path::new(relative);

    // 絶対パス指定(Windowsのドライブレター指定含む)を明示的に拒否する。
    if relative_path.is_absolute() || relative_path.components().any(|c| {
        matches!(c, std::path::Component::ParentDir | std::path::Component::Prefix(_))
    }) {
        return text_response(StatusCode::FORBIDDEN, "path traversal rejected");
    }

    let candidate: PathBuf = if relative.is_empty() {
        docroot.join("index.html")
    } else {
        docroot.join(relative_path)
    };

    let canonical_docroot = match docroot.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("docroot is not accessible: {e}"),
            )
        }
    };

    let canonical_candidate = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => return text_response(StatusCode::NOT_FOUND, "not found"),
    };

    if !canonical_candidate.starts_with(&canonical_docroot) {
        return text_response(StatusCode::FORBIDDEN, "path traversal rejected");
    }

    if canonical_candidate.is_dir() {
        return text_response(StatusCode::FORBIDDEN, "directory listing not permitted");
    }

    match std::fs::read(&canonical_candidate) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", guess_content_type(&canonical_candidate))
            .body(Full::new(Bytes::from(bytes)))
            .expect("static file response is always well-formed"),
        Err(_) => text_response(StatusCode::NOT_FOUND, "not found"),
    }
}

/// 拡張子から簡易的にContent-Typeを推定する。網羅的である必要はなく、
/// 主要な静的アセット種別をカバーすれば十分(ブラウザ側のsniffingで
/// 大半は補われる)。
fn guess_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// 拡張子から「静的アセットとして直接配信すべきか」を判定する。
/// PHP実行が必要なパス(`.php`)や拡張子無しのルーティング用パス
/// (`index.php`等へPHP側でフォールバックさせたいもの)はfalseを返す。
pub fn is_static_asset(request_path: &str) -> bool {
    let path = request_path.split('?').next().unwrap_or(request_path);
    matches!(
        Path::new(path).extension().and_then(|e| e.to_str()),
        Some(
            "css" | "js" | "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "woff" | "woff2"
                | "txt" | "map" | "mp4" | "webm" | "mp3"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_docroot() -> tempfile_like::TempDir {
        tempfile_like::TempDir::new()
    }

    // 依存追加を避けるための最小限の使い捨てTempDir実装(std::env::temp_dir
    // + プロセスID + カウンタでユニークなディレクトリを作り、Dropで削除する)。
    mod tempfile_like {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn new() -> Self {
                let n = COUNTER.fetch_add(1, Ordering::SeqCst);
                let path = std::env::temp_dir().join(format!(
                    "open-web-server-static-test-{}-{}",
                    std::process::id(),
                    n
                ));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        impl std::ops::Deref for TempDir {
            type Target = Path;
            fn deref(&self) -> &Path {
                &self.0
            }
        }
    }

    #[test]
    fn serves_existing_file() {
        let dir = make_docroot();
        fs::write(dir.join("hello.txt"), b"hello world").unwrap();

        let resp = serve(&dir, "/hello.txt");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn missing_file_is_404() {
        let dir = make_docroot();
        let resp = serve(&dir, "/nope.txt");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn dotdot_traversal_is_rejected() {
        let dir = make_docroot();
        fs::write(dir.join("hello.txt"), b"hello").unwrap();

        let resp = serve(&dir, "/../../../../etc/passwd");
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn encoded_traversal_style_paths_without_dotdot_stay_scoped() {
        // シンボリックリンク以外でdocroot外に出る経路が無いことの追加確認
        // (絶対パス指定を拒否する分岐の実証)。
        let dir = make_docroot();
        let resp = serve(&dir, "/C:/Windows/System32/drivers/etc/hosts");
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn directory_traversal_via_symlink_escape_is_rejected() {
        // symlinkがdocroot外を指す場合、canonicalize後のstarts_withチェックで
        // 弾かれることを確認する(Windows開発環境でシンボリックリンク作成
        // 権限が無い場合はテスト自体をスキップする——CI/開発者権限に依存する
        // 既知の制約として明記)。
        let dir = make_docroot();
        let outside = make_docroot();
        std::fs::write(outside.join("secret.txt"), b"top secret").unwrap();

        let link_path = dir.join("escape");
        #[cfg(unix)]
        let created = std::os::unix::fs::symlink(&*outside, &link_path).is_ok();
        #[cfg(windows)]
        let created = std::os::windows::fs::symlink_dir(&*outside, &link_path).is_ok();

        if !created {
            eprintln!("skipping symlink escape test: insufficient privilege to create symlinks");
            return;
        }

        let resp = serve(&dir, "/escape/secret.txt");
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn is_static_asset_classifies_extensions() {
        assert!(is_static_asset("/images/foo.png"));
        assert!(is_static_asset("/css/style.css?v=2"));
        assert!(!is_static_asset("/index.php"));
        assert!(!is_static_asset("/"));
    }
}
