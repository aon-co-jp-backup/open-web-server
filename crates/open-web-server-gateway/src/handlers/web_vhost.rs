//! 静的ファイル/PHPサイト向けvhostのディスパッチハンドラ。
//!
//! ルーティング方針(ユーザー指示に基づく工学的判断):
//! - 拡張子から明らかに静的アセット(`.css`/`.js`/画像等)と判定できる
//!   パスは、まず`static_files::serve`でdocrootから直接配信を試みる
//!   (実運用でのApache的な静的配信を優先するため)。
//! - 上記に該当しない、またはvhostがPHP無効な静的サイトでファイルが
//!   実在しない場合は、PHP有効なvhostであれば`php_server::PhpServerPool`が
//!   管理する`php -S`サブプロセスへリバースプロキシで委譲する
//!   (PHPの組み込みルーティング — `index.php`が存在すればディレクトリ
//!   ルートへのリクエストもそちらへ処理させる)。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};

use crate::proxy;
use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;
use crate::static_files;
use crate::web_vhost::{CompatMode, PhpMode, WebVhostConfig, WebVhostError};

pub async fn dispatch(
    state: Arc<AppState>,
    vhost: Arc<WebVhostConfig>,
    req: Request<Incoming>,
) -> Response<BoxBody> {
    let path = req.uri().path().to_string();

    if static_files::is_static_asset(&path) {
        let resp = static_files::serve(&vhost.docroot, &path);
        if resp.status() != StatusCode::NOT_FOUND {
            return resp;
        }
        // 静的ファイルとして見つからなければPHPへフォールバック(下記)。
    }

    if !vhost.php_enabled {
        return serve_static_vhost(&vhost.docroot, &path, vhost.compat_mode);
    }

    match &vhost.php_mode {
        PhpMode::BuiltinServer => match state.php_pool.ensure_running(&vhost.docroot).await {
            Ok(addr) => proxy::forward_to(&addr, req).await,
            Err(e) => text_response(
                StatusCode::BAD_GATEWAY,
                format!("failed to start php built-in server for this vhost: {e}"),
            ),
        },
        PhpMode::FastCgi { fastcgi_addr } => {
            dispatch_fastcgi(fastcgi_addr, &vhost.docroot, req).await
        }
    }
}

/// `PhpMode::FastCgi`向けの委譲。`fastcgi-client` featureが有効な場合のみ
/// 実際にphp-fpmへFastCGI経由で接続する(`php_fastcgi`参照)。無効な
/// ビルドでは正直に`501 Not Implemented`を返し、パニックや無言のフォール
/// バックはしない。
#[cfg(feature = "fastcgi-client")]
async fn dispatch_fastcgi(
    fastcgi_addr: &str,
    docroot: &std::path::Path,
    req: Request<Incoming>,
) -> Response<BoxBody> {
    crate::php_fastcgi::proxy_fastcgi(fastcgi_addr, docroot, req).await
}

#[cfg(not(feature = "fastcgi-client"))]
async fn dispatch_fastcgi(
    fastcgi_addr: &str,
    _docroot: &std::path::Path,
    _req: Request<Incoming>,
) -> Response<BoxBody> {
    text_response(
        StatusCode::NOT_IMPLEMENTED,
        format!(
            "this build was compiled without the 'fastcgi-client' feature; \
             cannot reach php-fpm at '{fastcgi_addr}'"
        ),
    )
}

/// PHP無効な静的サイトの配信を行う。Apache互換モードでは、リクエスト
/// されたファイルがdocroot配下に見つからない場合`index.html`へ
/// フォールバックする(`.htaccess`の`FallbackResource`相当のSPA的挙動)。
/// Nginx互換モードは既存通り、フォールバックせず素直に404を返す
/// (`try_files $uri $uri/ =404;`相当、既存動作との完全な後方互換)。
fn serve_static_vhost(
    docroot: &std::path::Path,
    path: &str,
    compat_mode: CompatMode,
) -> Response<BoxBody> {
    let resp = static_files::serve(docroot, path);
    if resp.status() == StatusCode::NOT_FOUND && compat_mode == CompatMode::Apache {
        return static_files::serve(docroot, "/index.html");
    }
    resp
}

/// `POST /admin/web-vhosts` — 静的ファイル/PHPサイト向けvhostを追加(または
/// 既存ホストを置き換え)る。既存の`tenant_router`(APIバックエンド用途)の
/// 管理APIと同じ認証(`handlers::tenants::check_admin_auth`)を再利用する。
pub async fn upsert_web_vhost(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = crate::handlers::tenants::check_admin_auth(&state, &req) {
        return resp;
    }

    let config: WebVhostConfig = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    state.web_vhosts.upsert(config).await;
    text_response(StatusCode::CREATED, "web vhost registered")
}

/// `DELETE /admin/web-vhosts/:host`
pub async fn remove_web_vhost(
    state: Arc<AppState>,
    req: &Request<Incoming>,
    host: &str,
) -> Response<BoxBody> {
    if let Err(resp) = crate::handlers::tenants::check_admin_auth(&state, req) {
        return resp;
    }

    match state.web_vhosts.remove(host).await {
        Ok(()) => text_response(StatusCode::OK, "web vhost removed"),
        Err(WebVhostError::NotFound(host)) => {
            text_response(StatusCode::NOT_FOUND, format!("host '{host}' not found"))
        }
    }
}

/// `GET /admin/web-vhosts` — 登録済みvhost一覧。
pub async fn list_web_vhosts(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = crate::handlers::tenants::check_admin_auth(&state, req) {
        return resp;
    }

    let list = state.web_vhosts.list().await;
    json_response(StatusCode::OK, &list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn make_docroot_with_index() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "open-web-server-webvhost-test-{}-{}",
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), b"<html>home</html>").unwrap();
        dir
    }

    #[test]
    fn nginx_compat_mode_returns_404_without_fallback() {
        let dir = make_docroot_with_index();
        let resp = serve_static_vhost(&dir, "/missing-page", CompatMode::Nginx);
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apache_compat_mode_falls_back_to_index_html() {
        let dir = make_docroot_with_index();
        let resp = serve_static_vhost(&dir, "/missing-page", CompatMode::Apache);
        assert_eq!(resp.status(), StatusCode::OK);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn both_modes_serve_existing_file_identically() {
        let dir = make_docroot_with_index();
        let nginx_resp = serve_static_vhost(&dir, "/index.html", CompatMode::Nginx);
        let apache_resp = serve_static_vhost(&dir, "/index.html", CompatMode::Apache);
        assert_eq!(nginx_resp.status(), StatusCode::OK);
        assert_eq!(apache_resp.status(), StatusCode::OK);
        std::fs::remove_dir_all(&dir).ok();
    }
}
