//! `POST /admin/ddns/setup-free-domain` — 無料DDNS(DuckDNS)による
//! サブドメイン取得〜自動更新のセットアップを1回のAPI呼び出しで完結
//! させる管理API(既存の`OPEN_WEB_SERVER_ADMIN_TOKEN`/`KeyGuardian`
//! 認証を再利用)。
//!
//! **正直な開示(スコープの境界)**: DuckDNSのアカウント自体(トークン
//! 発行)はユーザーがduckdns.orgでログイン(GitHub/Google/Reddit等の
//! OAuth)して取得する必要があり、それ自体は自動化できない——本
//! ソフトウェアが他社サービスの認証情報を代行取得することはしない
//! (既存のセキュリティ方針と整合)。「トークンさえあれば、その先は
//! 全自動」という現実的なスコープであり、このエンドポイントは
//! **トークンを受け取った後**の疎通確認と自動更新ループへの組み込みを
//! 担う。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::Deserialize;
#[cfg(feature = "ddns")]
use serde::Serialize;

use crate::handlers::tenants::check_admin_auth;
use crate::response::{read_json_body, text_response, BoxBody};
#[cfg(feature = "ddns")]
use crate::response::json_response;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SetupFreeDomainRequest {
    /// 希望サブドメイン名(`.duckdns.org`を除いた部分、例: `"myhost"`)。
    pub domain: String,
    /// duckdns.org上でユーザー自身が取得したトークン(このソフトウェアは
    /// 代行取得しない)。
    pub token: String,
}

#[cfg(feature = "ddns")]
#[derive(Serialize)]
pub struct SetupFreeDomainResponse {
    /// 完全なDuckDNSホスト名(例: `"myhost.duckdns.org"`)。
    pub full_hostname: String,
    /// 即時疎通確認(1回の更新試行)が成功したか。
    pub verified: bool,
    /// DuckDNS APIの生レスポンス(デバッグ・正直な開示のため、加工せず含める)。
    pub duckdns_raw_response: String,
    /// 案内メッセージ(自動更新ループへの組み込み状況・正直なスコープ説明)。
    pub message: String,
}

pub async fn setup_free_domain(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, &req) {
        return resp;
    }

    let payload: SetupFreeDomainRequest = match read_json_body(req).await {
        Ok(body) => body,
        Err(resp) => return resp,
    };

    if payload.domain.trim().is_empty() || payload.token.trim().is_empty() {
        return text_response(
            StatusCode::BAD_REQUEST,
            "both 'domain' and 'token' must be non-empty",
        );
    }

    #[cfg(feature = "ddns")]
    {
        let client = reqwest::Client::new();
        match crate::free_domain::update_duckdns(&client, &payload.domain, &payload.token, None)
            .await
        {
            Ok(result) => {
                let full_hostname = format!("{}.duckdns.org", payload.domain);
                let message = if result.ok {
                    format!(
                        "疎通確認に成功しました。'{full_hostname}' はこれ以降、\
                         OPEN_WEB_SERVER_DUCKDNS_DOMAIN='{}' と \
                         OPEN_WEB_SERVER_DUCKDNS_TOKEN を環境変数に設定して\
                         再起動すると、5分間隔の自動更新ループに組み込まれます\
                         (このAPI呼び出し自体は環境変数を永続化しません——\
                         プロセス起動時の設定のみが自動更新ループの対象です)。\
                         なお、DuckDNSアカウント/トークンの発行自体はduckdns.org\
                         へのユーザー自身のログインが必要であり、本ソフトウェアは\
                         それを代行しません。",
                        payload.domain
                    )
                } else {
                    format!(
                        "DuckDNS側が失敗を返しました('{full_hostname}')。トークンや\
                         サブドメイン名を再確認してください。生レスポンス: {}",
                        result.raw_body
                    )
                };
                let status = if result.ok {
                    StatusCode::OK
                } else {
                    StatusCode::BAD_GATEWAY
                };
                json_response(
                    status,
                    &SetupFreeDomainResponse {
                        full_hostname,
                        verified: result.ok,
                        duckdns_raw_response: result.raw_body,
                        message,
                    },
                )
            }
            Err(e) => text_response(
                StatusCode::BAD_GATEWAY,
                format!("failed to reach DuckDNS update API: {e}"),
            ),
        }
    }
    #[cfg(not(feature = "ddns"))]
    {
        text_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "this build was compiled without the 'ddns' feature; DuckDNS integration is unavailable",
        )
    }
}
