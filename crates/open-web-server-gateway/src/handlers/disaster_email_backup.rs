//! スタンドアロンのメール・ディザスタバックアップ管理API
//! (`disaster_email_backup` feature、任意有効化)。
//!
//! **設計方針(ユーザー指示、2026-07-25)**: `open-easy-web`の
//! `dist_sync.rs`と同じ「VPS同期先の登録有無に関わらず、メールアドレス
//! ひとつだけで有効化できる」という要件を、この`open-web-server`側でも
//! 満たす。既存の管理API(`check_admin_auth`、`x-admin-token`/Bearerキー
//! 併用)と同じ認証パターンをそのまま流用する——新しい認証方式は増やさない。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use open_web_server_ledger::DisasterEmailBackupConfig;
use serde::Serialize;

use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;

#[derive(Debug, Serialize)]
struct SimpleMessage {
    message_ja: &'static str,
    message_en: &'static str,
}

/// `POST /admin/disaster-email-backup` — メールアドレスひとつだけで
/// スタンドアロンのディザスタ・メールバックアップを有効化する。
/// VPS同期先・マルチリージョンレプリケータの設定は一切不要。
pub async fn set_disaster_email_backup(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = super::tenants::check_admin_auth(&state, &req) {
        return resp;
    }

    let body: DisasterEmailBackupConfig = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    state.ledger.set_disaster_email_backup(open_web_server_ledger::DisasterEmailBackup::new(body));

    json_response(
        StatusCode::OK,
        &SimpleMessage {
            message_ja: "ディザスタ用メール退避先を設定しました(他の同期・レプリケーション設定は不要です)。",
            message_en: "Disaster email backup destination configured (no other sync/replication setup required).",
        },
    )
}

/// `POST /admin/disaster-email-backup/verify` — SMTP接続の疎通確認のみ
/// (実際にメールは送信しない)。
pub async fn verify_disaster_email_backup(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = super::tenants::check_admin_auth(&state, &req) {
        return resp;
    }

    let Some(backup) = state.ledger.disaster_email_backup() else {
        return text_response(StatusCode::NOT_FOUND, "disaster email backup is not configured yet");
    };

    match tokio::task::spawn_blocking(move || backup.ensure_ready()).await {
        Ok(Ok(())) => json_response(
            StatusCode::OK,
            &SimpleMessage {
                message_ja: "SMTP接続を確認できました。",
                message_en: "SMTP connectivity check succeeded.",
            },
        ),
        Ok(Err(e)) => text_response(StatusCode::SERVICE_UNAVAILABLE, &format!("SMTP connectivity check failed: {e}")),
        Err(e) => text_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("verification task panicked: {e}")),
    }
}
