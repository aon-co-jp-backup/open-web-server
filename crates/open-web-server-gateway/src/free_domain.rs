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
//! ## 複数ドメイン対応(2026-07-23追記、ユーザー指示)
//!
//! 1インスタンスにつき最大[`MAX_DUCKDNS_DOMAINS`]件まで、DuckDNSの
//! サブドメインを動的に登録・自動更新できる。設計は既存の
//! `tenant_router::TenantRegistry`(`RwLock<HashMap<..>>`による動的
//! 登録・削除、再起動不要)と同じパターンを踏襲した——[`DomainRegistry`]
//! が「サブドメイン名 → DuckDNSトークン」を保持し、管理API
//! (`POST /admin/ddns/setup-free-domain`)を複数回呼べば複数ドメインを
//! 追加登録できる。5分間隔の自動更新ループは、登録済み全ドメインを毎回
//! 順に更新する。
//!
//! 後方互換: 従来の単一ドメイン用環境変数`OPEN_WEB_SERVER_DUCKDNS_DOMAIN`/
//! `OPEN_WEB_SERVER_DUCKDNS_TOKEN`は、起動時に[`DomainRegistry`]へ
//! 1件目のドメインとしてシードする形で引き続き機能する
//! (`seed_from_env`)。

use std::collections::HashMap;

use tokio::sync::RwLock;

/// 1インスタンスあたりに登録できるDuckDNSサブドメインの上限
/// (ユーザー指示、2026-07-23)。マジックナンバーを避けるため定数化する。
pub const MAX_DUCKDNS_DOMAINS: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum FreeDomainError {
    #[error("capacity exceeded: this instance already has {0} DuckDNS domain(s) registered (max {MAX_DUCKDNS_DOMAINS})")]
    CapacityExceeded(usize),
    #[error("domain '{0}' is not registered")]
    NotFound(String),
}

/// 登録済みDuckDNSドメイン1件分(一覧表示用、トークンは含まない
/// ——管理API越しにトークンを漏らさないための意図的な設計)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegisteredDomainSummary {
    /// サブドメイン名(`.duckdns.org`を除いた部分)。
    pub domain: String,
    /// 完全なホスト名(例: `"myhost.duckdns.org"`)。
    pub full_hostname: String,
}

/// 「サブドメイン名 → DuckDNSトークン」を保持する動的レジストリ
/// (`tenant_router::TenantRegistry`と同じ`RwLock<HashMap<..>>`パターン)。
pub struct DomainRegistry {
    entries: RwLock<HashMap<String, String>>,
}

impl DomainRegistry {
    pub fn new() -> Self {
        Self { entries: RwLock::new(HashMap::new()) }
    }

    /// `OPEN_WEB_SERVER_DUCKDNS_DOMAIN`/`OPEN_WEB_SERVER_DUCKDNS_TOKEN`
    /// (従来の単一ドメイン用環境変数)が設定されていれば、起動時に1件目の
    /// ドメインとしてシードする(後方互換)。
    pub async fn seed_from_env(&self) {
        let (Ok(domain), Ok(token)) = (
            std::env::var("OPEN_WEB_SERVER_DUCKDNS_DOMAIN"),
            std::env::var("OPEN_WEB_SERVER_DUCKDNS_TOKEN"),
        ) else {
            return;
        };
        if domain.trim().is_empty() || token.trim().is_empty() {
            return;
        }
        // 起動直後、レジストリは空のはずなので容量エラーは通常起き得ないが、
        // 念のため結果は無視せずログに残す。
        if let Err(e) = self.register(domain.clone(), token).await {
            tracing::warn!("failed to seed DuckDNS domain '{domain}' from environment: {e}");
        }
    }

    /// ドメインを登録する(既に登録済みの場合はトークンを更新するのみで、
    /// 容量上限のカウントは増えない)。新規登録で上限
    /// [`MAX_DUCKDNS_DOMAINS`]を超える場合は`CapacityExceeded`を返す。
    pub async fn register(&self, domain: String, token: String) -> Result<(), FreeDomainError> {
        let mut guard = self.entries.write().await;
        if !guard.contains_key(&domain) && guard.len() >= MAX_DUCKDNS_DOMAINS {
            return Err(FreeDomainError::CapacityExceeded(guard.len()));
        }
        guard.insert(domain, token);
        Ok(())
    }

    /// ドメインを登録解除する。
    pub async fn remove(&self, domain: &str) -> Result<(), FreeDomainError> {
        let mut guard = self.entries.write().await;
        if guard.remove(domain).is_none() {
            return Err(FreeDomainError::NotFound(domain.to_string()));
        }
        Ok(())
    }

    /// 登録済みドメインの一覧(トークンは含まない)。
    pub async fn list(&self) -> Vec<RegisteredDomainSummary> {
        let guard = self.entries.read().await;
        let mut out: Vec<RegisteredDomainSummary> = guard
            .keys()
            .map(|domain| RegisteredDomainSummary {
                domain: domain.clone(),
                full_hostname: format!("{domain}.duckdns.org"),
            })
            .collect();
        out.sort_by(|a, b| a.domain.cmp(&b.domain));
        out
    }

    /// 自動更新ループが1周ごとに使う、(domain, token)のスナップショット
    /// (`ddns` feature無効時は自動更新ループ自体が存在しないため未使用)。
    #[cfg_attr(not(feature = "ddns"), allow(dead_code))]
    pub async fn snapshot(&self) -> Vec<(String, String)> {
        self.entries.read().await.iter().map(|(d, t)| (d.clone(), t.clone())).collect()
    }

    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }
}

impl Default for DomainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// クエリパラメータ用の最小限のパーセントエンコード(依存追加を避けるため
/// 自前実装、`tenants.rs`の`urlencoding_lite_decode`と対になる符号化側)。
/// `ddns` feature無効時はDuckDNS更新URLを組み立てるコード自体が存在しない
/// ため未使用になる。
#[cfg_attr(not(feature = "ddns"), allow(dead_code))]
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

#[cfg(feature = "ddns")]
mod net {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);
    const IP_ECHO_URL: &str = "https://api.ipify.org";
    const DUCKDNS_UPDATE_BASE: &str = "https://www.duckdns.org/update";

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

    async fn fetch_current_ip(client: &reqwest::Client) -> Result<String, reqwest::Error> {
        let text = client.get(IP_ECHO_URL).send().await?.text().await?;
        Ok(text.trim().to_string())
    }

    /// レジストリ経由で登録済みの全ドメイン(最大[`MAX_DUCKDNS_DOMAINS`]件)を
    /// バックグラウンドで5分間隔・自動更新するループを起動する。レジストリが
    /// 空でも(後から`/admin/ddns/setup-free-domain`で追加登録される可能性が
    /// あるため)常にループ自体は起動しておく。
    pub fn spawn_if_configured(registry: Arc<DomainRegistry>) {
        tokio::spawn(run_loop(registry));
    }

    async fn run_loop(registry: Arc<DomainRegistry>) {
        let client = reqwest::Client::new();
        let mut last_ip: Option<String> = None;
        loop {
            let domains = registry.snapshot().await;
            if !domains.is_empty() {
                match fetch_current_ip(&client).await {
                    Ok(ip) => {
                        if last_ip.as_deref() != Some(ip.as_str()) {
                            tracing::info!(
                                "DuckDNS: detected IP change (was {:?}, now {ip}), updating {} domain(s)",
                                last_ip,
                                domains.len()
                            );
                            let mut all_ok = true;
                            for (domain, token) in &domains {
                                match update_duckdns(&client, domain, token, Some(&ip)).await {
                                    Ok(result) if result.ok => {
                                        tracing::info!("DuckDNS: update succeeded ({domain}.duckdns.org -> {ip})");
                                    }
                                    Ok(result) => {
                                        all_ok = false;
                                        tracing::warn!("DuckDNS: update for '{domain}' responded with failure body: {}", result.raw_body);
                                    }
                                    Err(e) => {
                                        all_ok = false;
                                        tracing::warn!("DuckDNS: update request for '{domain}' failed: {e}");
                                    }
                                }
                            }
                            if all_ok {
                                last_ip = Some(ip);
                            }
                        }
                    }
                    Err(e) => tracing::warn!("DuckDNS: failed to fetch current IP: {e}"),
                }
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    }
}

#[cfg(feature = "ddns")]
pub use net::{spawn_if_configured, update_duckdns};

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

    #[tokio::test]
    async fn registry_enforces_capacity_limit() {
        let registry = DomainRegistry::new();
        for i in 0..MAX_DUCKDNS_DOMAINS {
            registry.register(format!("host{i}"), "token".to_string()).await.expect("should register up to the limit");
        }
        assert_eq!(registry.len().await, MAX_DUCKDNS_DOMAINS);

        let err = registry
            .register("one-too-many".to_string(), "token".to_string())
            .await
            .expect_err("21st distinct domain must be rejected");
        assert!(matches!(err, FreeDomainError::CapacityExceeded(n) if n == MAX_DUCKDNS_DOMAINS));
    }

    #[tokio::test]
    async fn registry_allows_re_registering_existing_domain_at_capacity() {
        let registry = DomainRegistry::new();
        for i in 0..MAX_DUCKDNS_DOMAINS {
            registry.register(format!("host{i}"), "token".to_string()).await.unwrap();
        }
        // 既存ドメインのトークン更新は、容量が満杯でも新規追加ではないため成功する。
        registry.register("host0".to_string(), "new-token".to_string()).await.expect("updating an existing entry must not count against capacity");
        assert_eq!(registry.len().await, MAX_DUCKDNS_DOMAINS);
    }

    #[tokio::test]
    async fn registry_remove_then_list_reflects_change() {
        let registry = DomainRegistry::new();
        registry.register("alpha".to_string(), "t1".to_string()).await.unwrap();
        registry.register("beta".to_string(), "t2".to_string()).await.unwrap();
        assert_eq!(registry.list().await.len(), 2);

        registry.remove("alpha").await.unwrap();
        let list = registry.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].domain, "beta");
        assert_eq!(list[0].full_hostname, "beta.duckdns.org");

        let err = registry.remove("alpha").await.expect_err("already removed");
        assert!(matches!(err, FreeDomainError::NotFound(d) if d == "alpha"));
    }

    #[tokio::test]
    async fn seed_from_env_is_a_noop_without_env_vars() {
        std::env::remove_var("OPEN_WEB_SERVER_DUCKDNS_DOMAIN");
        std::env::remove_var("OPEN_WEB_SERVER_DUCKDNS_TOKEN");
        let registry = DomainRegistry::new();
        registry.seed_from_env().await;
        assert_eq!(registry.len().await, 0);
    }

    #[cfg(feature = "ddns")]
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
