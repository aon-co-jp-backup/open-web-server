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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::power_profile::{effective_poll_interval, PowerProfileRegistry};

const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// 1回のHTTPリクエストに対するタイムアウト(2026-07-29追記、RS-Sync側で
/// 見つかった実バグ——タイムアウト無しのHTTPクライアントが応答を待ち
/// 続けてループ全体が無言のまま止まっていた——の横展開)。この処理は
/// 元々`async`な`reqwest::Client`のため単独のタスクが詰まっても他の
/// tokioタスクは止まらないが、それでも「DDNS更新自体が永久に止まる」
/// リスクは残るため防御的に設定する。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// グローバルIPを取得するための、認証不要な公開エコーサービス。
/// (プレーンテキストで自分のIPだけを返す、広く使われている定番の1つ)。
const IP_ECHO_URL: &str = "https://api.ipify.org";

/// このバックグラウンドループが最後に1周した時刻(Unixエポック秒)。
/// 2026-07-29追記(ユーザー指示「このようなBUGが起きないか定期的に
/// 自動チェックする機能」、RS-Sync側のスケジューラ無言停止バグの
/// 横展開): `run_loop`が毎イテレーション更新する。5分間隔のループ
/// なので、10分(2周分の猶予)以上更新が無ければ異常とみなせる。
static LAST_TICK_UNIX: AtomicU64 = AtomicU64::new(0);

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `(healthy, seconds_since_tick)`。ループが一度も起動していない
/// (`OPEN_WEB_SERVER_DDNS_UPDATE_URL`未設定でループ自体がspawnされて
/// いない)場合は`healthy=true`を返す(そもそも無効機能なので異常では
/// ない、という判定)。
pub fn heartbeat_status() -> (bool, u64) {
    let last = LAST_TICK_UNIX.load(Ordering::Relaxed);
    if last == 0 {
        return (true, 0);
    }
    let elapsed = now_unix().saturating_sub(last);
    (elapsed < 600, elapsed)
}

/// 環境変数`OPEN_WEB_SERVER_DDNS_UPDATE_URL`が設定されていれば、
/// バックグラウンドタスクとして定期的(既定5分ごと)にグローバルIPを
/// 確認し、前回から変化していれば更新URLを叩く。設定が無ければ何もしない
/// (固定IP環境では不要な機能のため、既定で無効)。
/// `power_profile`は省メモリ/省電力プロファイル(2026-07-26追加、
/// `crate::power_profile`参照)——現在のプロファイルに応じて、下記
/// `run_loop`の待機間隔を**毎イテレーション読み直す**ことで、プロセスを
/// 再起動せずにポーリング頻度を変えられるようにする。
pub fn spawn_if_configured(power_profile: Arc<PowerProfileRegistry>) {
    let Ok(template) = std::env::var("OPEN_WEB_SERVER_DDNS_UPDATE_URL") else {
        return;
    };
    if !template.contains("{ip}") {
        tracing::warn!("OPEN_WEB_SERVER_DDNS_UPDATE_URL is set but doesn't contain '{{ip}}' placeholder; DDNS updates disabled");
        return;
    }
    tokio::spawn(run_loop(template, power_profile));
}

async fn run_loop(template: String, power_profile: Arc<PowerProfileRegistry>) {
    let client = reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap_or_else(|_| reqwest::Client::new());
    let mut last_ip: Option<String> = None;
    loop {
        LAST_TICK_UNIX.store(now_unix(), Ordering::Relaxed);
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
        // 起動時に一度だけ間隔を固定するのではなく、毎回`power_profile`の
        // 現在値を読み直す(省電力/常時電源接続プロファイルへの途中切替を
        // 次のイテレーションから即座に反映するため)。
        tokio::time::sleep(effective_poll_interval(&power_profile, CHECK_INTERVAL)).await;
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
        spawn_if_configured(Arc::new(PowerProfileRegistry::new()));
    }
}
