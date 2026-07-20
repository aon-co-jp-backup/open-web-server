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
use crate::web_vhost::{WebVhostConfig, WebVhostError};

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
        return static_files::serve(&vhost.docroot, &path);
    }

    match state.php_pool.ensure_running(&vhost.docroot).await {
        Ok(addr) => proxy::forward_to(&addr, req).await,
        Err(e) => text_response(
            StatusCode::BAD_GATEWAY,
            format!("failed to start php built-in server for this vhost: {e}"),
        ),
    }
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
