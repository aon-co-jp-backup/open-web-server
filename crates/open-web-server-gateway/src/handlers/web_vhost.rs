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
use crate::rewrite::{self, RewriteOutcome};
use crate::state::AppState;
use crate::static_files;
use crate::web_vhost::{CompatMode, PhpMode, WebVhostConfig, WebVhostError};

pub async fn dispatch(
    state: Arc<AppState>,
    vhost: Arc<WebVhostConfig>,
    mut req: Request<Incoming>,
) -> Response<BoxBody> {
    let original_path = req.uri().path().to_string();

    // Apache `.htaccess`のRewriteRule相当(`crate::rewrite`参照)。
    // 外部リダイレクトは即座に301を返す。内部リライトは、以後の静的
    // 配信・PHP委譲のいずれの経路でも書き換え後のパスを使うよう、
    // リクエストのURI自体を差し替える。
    let path = match rewrite::apply(&original_path, &vhost.rewrite_rules) {
        RewriteOutcome::Redirect(target) => {
            let mut resp = Response::builder().status(StatusCode::MOVED_PERMANENTLY).body(BoxBody::default()).unwrap();
            if let Ok(value) = hyper::header::HeaderValue::from_str(&target) {
                resp.headers_mut().insert(hyper::header::LOCATION, value);
            }
            return resp;
        }
        RewriteOutcome::Rewritten(new_path) => {
            if let Ok(new_uri) = new_path.parse::<hyper::Uri>() {
                *req.uri_mut() = new_uri;
            }
            req.uri().path().to_string()
        }
        RewriteOutcome::Unchanged => original_path,
    };

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

#[derive(serde::Deserialize)]
struct ImportVhostRequest {
    /// `"apache"`または`"nginx"`。
    format: String,
    /// 生の設定テキスト(`<VirtualHost>`ブロック、または`server {}`
    /// ブロック——ブロック自体を含んでいても中身だけでもどちらでもよい、
    /// `config_import`は行単位でディレクティブを拾うだけなので構造の
    /// ネストを気にしない)。
    config: String,
}

/// `POST /admin/web-vhosts/import` — 実際のApache/Nginx設定ファイルから
/// vhost定義の基本部分(ホスト名・ドキュメントルート・PHP-FPM接続先)を
/// 読み取って登録する(2026-08-03新設、改善計画「(3) 実設定ファイルの
/// パース/インポート」対応、ユーザー指示によりスコープを基本部分のみに
/// 限定)。パース成功時は通常の`upsert`と同じ経路で登録される
/// (`compat_mode`は既定値〈Nginx互換〉、`rewrite_rules`は空——設定
/// ファイルの`RewriteRule`/`rewrite`ディレクティブ自体は今回のスコープ外、
/// 必要なら登録後に`PUT .../compat-mode`や`upsert`で追加設定する)。
pub async fn import_web_vhost(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = crate::handlers::tenants::check_admin_auth(&state, &req) {
        return resp;
    }

    let body: ImportVhostRequest = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    let parsed = match body.format.to_lowercase().as_str() {
        "apache" => crate::config_import::parse_apache_vhost(&body.config),
        "nginx" => crate::config_import::parse_nginx_server(&body.config),
        other => {
            return text_response(
                StatusCode::BAD_REQUEST,
                format!("unknown format '{other}', expected 'apache' or 'nginx'"),
            )
        }
    };

    match parsed {
        Ok(config) => {
            let host = config.host.clone();
            state.web_vhosts.upsert(config).await;
            text_response(StatusCode::CREATED, format!("web vhost '{host}' imported"))
        }
        Err(e) => text_response(StatusCode::BAD_REQUEST, format!("failed to parse config: {e}")),
    }
}

#[derive(serde::Deserialize)]
struct UpdateCompatModeRequest {
    compat_mode: CompatMode,
}

/// `PUT /admin/web-vhosts/:host/compat-mode` — 既存vhostの
/// Apache互換/Nginx互換モードだけを変更する(2026-08-03新設、ユーザー
/// 指示「Apache/Nginxのヴァーチャルホストプロファイルはどちらでも
/// 読めていつでも両方対応可能に」)。`docroot`等の他フィールドを再送する
/// 必要がなく、稼働中いつでも安全に切り替えられる。ホスト未登録なら404。
pub async fn update_compat_mode(
    state: Arc<AppState>,
    req: Request<Incoming>,
    host: &str,
) -> Response<BoxBody> {
    if let Err(resp) = crate::handlers::tenants::check_admin_auth(&state, &req) {
        return resp;
    }

    let body: UpdateCompatModeRequest = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    match state.web_vhosts.set_compat_mode(host, body.compat_mode).await {
        Ok(()) => text_response(StatusCode::OK, "compat mode updated"),
        Err(WebVhostError::NotFound(host)) => {
            text_response(StatusCode::NOT_FOUND, format!("host '{host}' not found"))
        }
    }
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
