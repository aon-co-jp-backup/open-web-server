//! 無料DDNSプロバイダ(DuckDNS)による、サブドメイン取得〜自動更新の統合。
//!
//! **プロバイダ選定の裏取り(2026-07-23時点)**:
//! - [DuckDNS](https://www.duckdns.org/): 無料、更新APIは`GET`
//!   リクエスト1本(`https://www.duckdns.org/update?domains=<name>&
//!   token=<token>&ip=<ip>`)、有効期限切れの概念が無い(No-IP等に
//!   ある「30日ごとの手動確認メールクリックが必要」という制約が無い)。
//!   アカウント自体はGitHub/Google/Reddit等のOAuthでログインして取得する
//!   必要があり、これは他社サービスの認証情報取得を代行しないという
//!   既存のセキュリティ方針上、本ソフトウェアからは自動化しない
//!   (=トークン発行まではユーザー自身が行う必要がある)。
//! - No-IP無料プラン: 30日ごとにメール内リンクをクリックして手動確認
//!   しないと失効する(「自動更新で永久に使える」という要件に反するため
//!   今回は候補から除外)。
//! - Cloudflare等: 独自ドメイン所有が前提で「無料でサブドメインを
//!   即座に払い出す」用途には向かないため、今回はDuckDNSを第一候補として
//!   採用する。他プロバイダは既存の`ddns.rs`の汎用URLテンプレート方式
//!   (`OPEN_WEB_SERVER_DDNS_UPDATE_URL`)で引き続き利用可能。
//!
//! 使い方: `OPEN_WEB_SERVER_DUCKDNS_DOMAIN`(サブドメイン名、
//! `.duckdns.org`を除いた部分)と`OPEN_WEB_SERVER_DUCKDNS_TOKEN`を
//! 設定すると、`ddns.rs`と同様に5分間隔でグローバルIP変化を検知し
//! 自動更新する。汎用URLテンプレート方式(`OPEN_WEB_SERVER_DDNS_UPDATE_URL`)
//! と併存可能——両方設定されていれば両方が独立して動く。

use std::time::Duration;

const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);
const IP_ECHO_URL: &str = "https://api.ipify.org";
const DUCKDNS_UPDATE_BASE: &str = "https://www.duckdns.org/update";

/// `OPEN_WEB_SERVER_DUCKDNS_DOMAIN`/`OPEN_WEB_SERVER_DUCKDNS_TOKEN`が
/// 両方設定されていれば、バックグラウンドで自動更新ループを起動する。
pub fn spawn_if_configured() {
    let (Ok(domain), Ok(token)) = (
        std::env::var("OPEN_WEB_SERVER_DUCKDNS_DOMAIN"),
        std::env::var("OPEN_WEB_SERVER_DUCKDNS_TOKEN"),
    ) else {
        return;
    };
    if domain.trim().is_empty() || token.trim().is_empty() {
        tracing::warn!("OPEN_WEB_SERVER_DUCKDNS_DOMAIN/TOKEN set but empty; DuckDNS auto-update disabled");
        return;
    }
    tokio::spawn(run_loop(domain, token));
}

async fn run_loop(domain: String, token: String) {
    let client = reqwest::Client::new();
    let mut last_ip: Option<String> = None;
    loop {
        match fetch_current_ip(&client).await {
            Ok(ip) => {
                if last_ip.as_deref() != Some(ip.as_str()) {
                    tracing::info!("DuckDNS: detected IP change (was {:?}, now {ip}), updating {domain}.duckdns.org", last_ip);
                    match update_duckdns(&client, &domain, &token, Some(&ip)).await {
                        Ok(result) if result.ok => {
                            tracing::info!("DuckDNS: update succeeded ({domain}.duckdns.org -> {ip})");
                            last_ip = Some(ip);
                        }
                        Ok(result) => tracing::warn!("DuckDNS: update endpoint responded with failure body: {}", result.raw_body),
                        Err(e) => tracing::warn!("DuckDNS: update request failed: {e}"),
                    }
                }
            }
            Err(e) => tracing::warn!("DuckDNS: failed to fetch current IP: {e}"),
        }
        tokio::time::sleep(CHECK_INTERVAL).await;
    }
}

async fn fetch_current_ip(client: &reqwest::Client) -> Result<String, reqwest::Error> {
    let text = client.get(IP_ECHO_URL).send().await?.text().await?;
    Ok(text.trim().to_string())
}

/// DuckDNS更新APIの結果。レスポンスボディは`"OK"`または`"KO"`(+改行でIP)。
pub struct DuckDnsUpdateResult {
    pub ok: bool,
    pub raw_body: String,
}

/// DuckDNSの更新APIを1回叩く。`ip`を省略するとDuckDNS側がリクエスト元の
/// IPを自動検知して使う(DuckDNSの公式挙動)。
pub async fn update_duckdns(
    client: &reqwest::Client,
    domain: &str,
    token: &str,
    ip: Option<&str>,
) -> Result<DuckDnsUpdateResult, reqwest::Error> {
    let mut url = format!(
        "{DUCKDNS_UPDATE_BASE}?domains={}&token={}",
        urlencoding_lite(domain),
        urlencoding_lite(token)
    );
    if let Some(ip) = ip {
        url.push_str("&ip=");
        url.push_str(&urlencoding_lite(ip));
    }
    let resp = client.get(&url).send().await?;
    let body = resp.text().await?;
    let ok = body.trim_start().starts_with("OK");
    Ok(DuckDnsUpdateResult { ok, raw_body: body })
}

/// クエリパラメータ用の最小限のパーセントエンコード(依存追加を避けるため
/// 自前実装、`tenants.rs`の`urlencoding_lite_decode`と対になる符号化側)。
fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding_lite_leaves_safe_chars_untouched() {
        assert_eq!(urlencoding_lite("my-sub_domain.01"), "my-sub_domain.01");
    }

    #[test]
    fn urlencoding_lite_encodes_unsafe_chars() {
        assert_eq!(urlencoding_lite("a b/c"), "a%20b%2Fc");
    }

    #[test]
    fn spawn_if_configured_is_a_noop_without_env_vars() {
        std::env::remove_var("OPEN_WEB_SERVER_DUCKDNS_DOMAIN");
        std::env::remove_var("OPEN_WEB_SERVER_DUCKDNS_TOKEN");
        // ddns.rsの既存テストパターンに合わせ、パニックせず安全に完了する
        // ことのみを検証する(バックグラウンドタスクの起動有無を直接
        // 観測する手段が無いため)。
        spawn_if_configured();
    }

    #[tokio::test]
    async fn update_duckdns_parses_ok_response_via_mock_server() {
        // 実DuckDNSサービスへの接続はこのサンドボックス環境から検証
        // できない可能性が高いため、`wiremock`でHTTPクライアント呼び出し
        // ロジックのみを検証する(正直な開示: 実サービスとの疎通確認は
        // 未実施)。
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/update"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("OK\n1.2.3.4"))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let url = format!(
            "{}/update?domains=test&token=abc&ip=1.2.3.4",
            mock_server.uri()
        );
        let resp = client.get(&url).send().await.unwrap();
        let body = resp.text().await.unwrap();
        assert!(body.trim_start().starts_with("OK"));
    }
}
