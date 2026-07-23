//! `GET /admin/sftp/connection-info` — 「今すぐ何を入力すればSFTP接続できるか」
//! を1回のAPI呼び出しで確認できるようにするヘルパー(既存の
//! `OPEN_WEB_SERVER_ADMIN_TOKEN`/`KeyGuardian`認証を再利用)。
//!
//! DDNSホスト名(`OPEN_WEB_SERVER_DDNS_UPDATE_URL`のホスト名部分は
//! プロバイダ固有なので推測しない)ではなく、シンプルに直近取得した
//! グローバルIPを`api.ipify.org`へその場で問い合わせて返す。DDNSで
//! 固定ホスト名を運用している場合は`OPEN_WEB_SERVER_SFTP_PUBLIC_HOST`
//! で明示的に上書きできる。

use std::sync::Arc;

use hyper::{Request, Response, StatusCode};
use hyper::body::Incoming;
use serde::Serialize;

use crate::handlers::tenants::check_admin_auth;
use crate::response::{json_response, BoxBody};
use crate::state::AppState;

#[derive(Serialize)]
pub struct SftpConnectionInfo {
    /// SFTP接続に使うホスト名(`OPEN_WEB_SERVER_SFTP_PUBLIC_HOST`優先、
    /// 未設定なら現在検知できたグローバルIP)。
    pub host: String,
    /// SFTPサーバーがbindしているポート(`OPEN_WEB_SERVER_SFTP_BIND`から抽出)。
    pub port: Option<u16>,
    /// 現在このプロセスから見えているグローバルIP(取得できた場合のみ)。
    pub detected_public_ip: Option<String>,
    /// コピペで使える接続コマンド例。
    pub example_command: Option<String>,
    /// SFTPサーバー自体が有効化されているか(`OPEN_WEB_SERVER_SFTP_BIND`の有無)。
    pub sftp_enabled: bool,
}

pub async fn connection_info(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }

    let bind = std::env::var("OPEN_WEB_SERVER_SFTP_BIND").ok();
    let sftp_enabled = bind.is_some();
    let port = bind
        .as_deref()
        .and_then(|b| b.rsplit(':').next())
        .and_then(|p| p.parse::<u16>().ok());

    let detected_public_ip = fetch_public_ip().await;

    let host = std::env::var("OPEN_WEB_SERVER_SFTP_PUBLIC_HOST")
        .ok()
        .or_else(|| detected_public_ip.clone());

    let example_command = match (&host, port) {
        (Some(h), Some(p)) => Some(format!("sftp -P {p} user@{h}")),
        _ => None,
    };

    let info = SftpConnectionInfo {
        host: host.unwrap_or_else(|| "(unknown - set OPEN_WEB_SERVER_SFTP_PUBLIC_HOST or check connectivity)".to_string()),
        port,
        detected_public_ip,
        example_command,
        sftp_enabled,
    };

    if !sftp_enabled {
        return json_response(StatusCode::OK, &info);
    }

    json_response(StatusCode::OK, &info)
}

async fn fetch_public_ip() -> Option<String> {
    #[cfg(feature = "ddns")]
    {
        let client = reqwest::Client::new();
        let resp = client.get("https://api.ipify.org").send().await.ok()?;
        let text = resp.text().await.ok()?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
    #[cfg(not(feature = "ddns"))]
    {
        // `ddns` featureが無効な場合は`reqwest`依存が無いため、外部IP検知は
        // 行わない(補助情報が取れないだけで、エンドポイント自体は動く)。
        None
    }
}
