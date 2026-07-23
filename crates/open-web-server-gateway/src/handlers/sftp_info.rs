//! `GET /admin/sftp/connection-info` — 「今すぐ何を入力すればSFTP接続できるか」
//! を1回のAPI呼び出しで確認できるようにするヘルパー(既存の
//! `OPEN_WEB_SERVER_ADMIN_TOKEN`/`KeyGuardian`認証を再利用)。
//!
//! ホスト名の優先順位(2026-07-23改修、複数DuckDNSドメイン対応との連動):
//! 1. `OPEN_WEB_SERVER_SFTP_PUBLIC_HOST`(明示的な手動指定、最優先)
//! 2. `?host=<domain>`クエリパラメータで指定された、登録済みDuckDNS
//!    ドメイン(`.duckdns.org`を補完して返す)——1インスタンスに複数
//!    ドメインが登録されている場合に、どれをSFTP接続用に使うか選べる。
//! 3. 登録済みDuckDNSドメインが1件以上あれば、その先頭(辞書順)のもの
//!    (`?host=`未指定時の既定値)——固定IPが無い環境でも「一度設定すれば
//!    変わらない」ホスト名として、その場で取得した生グローバルIPより
//!    実用上ずっと有用なため優先する。
//! 4. その場で`api.ipify.org`へ問い合わせて取得した生グローバルIP
//!    (DDNSを何も登録していない場合のフォールバック)
//!
//! レスポンスには、選ばれなかった分も含め登録済み全DuckDNSドメインの
//! ホスト名一覧(`available_duckdns_domains`)を常に含める。

use std::sync::Arc;

use hyper::{Request, Response, StatusCode};
use hyper::body::Incoming;
use serde::Serialize;

use crate::handlers::tenants::check_admin_auth;
use crate::response::{json_response, BoxBody};
use crate::state::AppState;

#[derive(Serialize)]
pub struct SftpConnectionInfo {
    /// SFTP接続に使うホスト名(優先順位はモジュールdoc参照)。
    pub host: String,
    /// SFTPサーバーがbindしているポート(`OPEN_WEB_SERVER_SFTP_BIND`から抽出)。
    pub port: Option<u16>,
    /// 現在このプロセスから見えているグローバルIP(取得できた場合のみ)。
    pub detected_public_ip: Option<String>,
    /// コピペで使える接続コマンド例。
    pub example_command: Option<String>,
    /// SFTPサーバー自体が有効化されているか(`OPEN_WEB_SERVER_SFTP_BIND`の有無)。
    pub sftp_enabled: bool,
    /// このインスタンスに登録済みの全DuckDNSドメイン(フルホスト名、
    /// `?host=`で選択できる候補一覧)。
    pub available_duckdns_domains: Vec<String>,
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

    let registered_domains = state.free_domains.list().await;
    let available_duckdns_domains: Vec<String> =
        registered_domains.iter().map(|d| d.full_hostname.clone()).collect();

    let requested_host = req.uri().query().and_then(|q| {
        q.split('&').find_map(|kv| kv.strip_prefix("host=")).map(str::to_string)
    });

    let selected_duckdns_host = match requested_host {
        // 明示的にクエリで選ばれたドメインが、実際に登録済みか確認する
        // (未登録のホスト名を無条件に信じない)。
        Some(requested) => registered_domains
            .iter()
            .find(|d| d.domain == requested || d.full_hostname == requested)
            .map(|d| d.full_hostname.clone()),
        // 未指定時は、登録済みドメインのうち先頭(辞書順)を既定値とする。
        None => registered_domains.first().map(|d| d.full_hostname.clone()),
    };

    let host = std::env::var("OPEN_WEB_SERVER_SFTP_PUBLIC_HOST")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .or(selected_duckdns_host)
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
        available_duckdns_domains,
    };

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
