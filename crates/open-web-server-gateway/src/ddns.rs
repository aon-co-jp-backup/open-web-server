//! 固定IPを持たない自宅サーバー等向けの、簡易DDNS(Dynamic DNS)更新。
//!
//! **正直な開示**: 特定のDDNSプロバイダ(No-IP・DuckDNS・Cloudflare等)の
//! 専用APIを個別に実装するのではなく、**汎用のURLテンプレート方式**を
//! 採用している——多くのDDNSプロバイダは`GET`リクエスト1本で更新できる
//! シンプルなAPIを持つため(`https://provider/update?hostname=X&myip=Y`
//! のような形)、そのURLを環境変数でそのまま指定してもらう設計にした。
//! これにより「対応プロバイダ一覧」を保守する必要が無い代わりに、
//! ユーザー自身がプロバイダのドキュメントからURL形式を確認する必要がある。
//!
//! 使い方: `OPEN_WEB_SERVER_DDNS_UPDATE_URL`に、現在のグローバルIPを
//! 埋め込みたい箇所を`{ip}`と書いたURLを設定する。例(DuckDNS):
//! `https://www.duckdns.org/update?domains=myhost&token=xxxx&ip={ip}`

use std::time::Duration;

const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// グローバルIPを取得するための、認証不要な公開エコーサービス。
/// (プレーンテキストで自分のIPだけを返す、広く使われている定番の1つ)。
const IP_ECHO_URL: &str = "https://api.ipify.org";

/// 環境変数`OPEN_WEB_SERVER_DDNS_UPDATE_URL`が設定されていれば、
/// バックグラウンドタスクとして定期的(既定5分ごと)にグローバルIPを
/// 確認し、前回から変化していれば更新URLを叩く。設定が無ければ何もしない
/// (固定IP環境では不要な機能のため、既定で無効)。
pub fn spawn_if_configured() {
    let Ok(template) = std::env::var("OPEN_WEB_SERVER_DDNS_UPDATE_URL") else {
        return;
    };
    if !template.contains("{ip}") {
        tracing::warn!("OPEN_WEB_SERVER_DDNS_UPDATE_URL is set but doesn't contain '{{ip}}' placeholder; DDNS updates disabled");
        return;
    }
    tokio::spawn(run_loop(template));
}

async fn run_loop(template: String) {
    let client = reqwest::Client::new();
    let mut last_ip: Option<String> = None;
    loop {
        match fetch_current_ip(&client).await {
            Ok(ip) => {
                if last_ip.as_deref() != Some(ip.as_str()) {
                    tracing::info!("DDNS: detected IP change (was {:?}, now {ip}), updating", last_ip);
                    match update_ddns(&client, &template, &ip).await {
                        Ok(status) if status.is_success() => {
                            tracing::info!("DDNS: update succeeded (HTTP {status})");
                            last_ip = Some(ip);
                        }
                        Ok(status) => tracing::warn!("DDNS: update endpoint returned HTTP {status}"),
                        Err(e) => tracing::warn!("DDNS: update request failed: {e}"),
                    }
                }
            }
            Err(e) => tracing::warn!("DDNS: failed to fetch current IP: {e}"),
        }
        tokio::time::sleep(CHECK_INTERVAL).await;
    }
}

async fn fetch_current_ip(client: &reqwest::Client) -> Result<String, reqwest::Error> {
    let text = client.get(IP_ECHO_URL).send().await?.text().await?;
    Ok(text.trim().to_string())
}

async fn update_ddns(client: &reqwest::Client, template: &str, ip: &str) -> Result<reqwest::StatusCode, reqwest::Error> {
    let url = template.replace("{ip}", ip);
    let resp = client.get(&url).send().await?;
    Ok(resp.status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_template_substitution_replaces_placeholder() {
        let template = "https://example.com/update?ip={ip}&host=test";
        let expected = "https://example.com/update?ip=203.0.113.5&host=test";
        assert_eq!(template.replace("{ip}", "203.0.113.5"), expected);
    }

    #[test]
    fn spawn_if_configured_is_a_noop_without_env_var() {
        std::env::remove_var("OPEN_WEB_SERVER_DDNS_UPDATE_URL");
        // パニックしない・何も起動しないことだけを確認(バックグラウンド
        // タスクの起動有無を直接観測する手段が無いため、呼び出しが
        // 安全に完了することのみを検証する)。
        spawn_if_configured();
    }
}
