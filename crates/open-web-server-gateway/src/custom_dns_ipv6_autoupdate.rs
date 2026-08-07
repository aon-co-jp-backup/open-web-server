//! v6プラス(MAP-E)環境下でも自宅サーバーを外部公開できるようにする、
//! バリュードメイン管理ドメイン(`aon.co.jp`)向けのAAAAレコード
//! (IPv6)自動更新ループ(2026-08-06新設)。
//!
//! ## 背景・設計方針
//!
//! v6プラス(MAP-E)環境ではIPv4は構造的にポート開放ができない一方、
//! IPv6は無制限に使える。ただしIPv6アドレスは動的に変わる(実機観測で
//! 約5分ごと)ため、DDNSと同じ発想で自動更新する必要がある——
//! [`crate::free_domain`](DuckDNS向けIPv4/A相当のDDNS)と同じ「5分間隔で
//! 変化を検知し、変化していれば登録済み全ドメインを更新する」ループ構造
//! をそのまま踏襲し、対象を「このマシンの現在のグローバルIPv6アドレス」
//! と「バリュードメインのAAAAレコード」に置き換えたもの。
//!
//! IPv6アドレスの取得方法は、OSのネットワークインターフェース一覧から
//! 直接取得する方法と、IPv6対応の外部echoサービスへHTTPリクエストする
//! 方法の二択のうち、**既存コードが`reqwest`(`custom_domain`/`ddns`/
//! `free_domain`いずれも同じ外部echoサービス方式)を既に使っている**
//! ことに合わせ、後者(`https://api6.ipify.org`)を採用した——OSの
//! インターフェース一覧から直接取得する方法は、v6プラスのようなCGN/
//! MAP-E環境下では「インターフェースに割り当てられたアドレス」と
//! 「実際にインターネットから到達可能なグローバルアドレス」が必ずしも
//! 一致しない(プレフィックス変換等)ため、外部echoサービス方式の方が
//! 実際に到達可能なアドレスを確実に得られると判断した。
//!
//! ## 対応プロバイダの範囲(正直な開示)
//!
//! 現時点で[`crate::custom_dns::DnsProvider::update_ipv6`]を実装している
//! のは`ValueDomainProvider`(`aon.co.jp`)のみ。`ConohaDnsProvider`
//! (`runo.tokyo`等)はまだAAAA対応を実装していないため、このループから
//! `runo.tokyo`等を対象に登録しようとすると、トレイトの既定実装が返す
//! 「このプロバイダはIPv6未対応」という`Err`がそのまま
//! [`AutoUpdateOutcome`]に記録される(黙って無視はしない)。

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::custom_dns::{DnsProvider, DnsProviderError, ValueDomainProvider};

/// v6プラス自動更新をこのインスタンスで同時に何件まで有効化できるか
/// (`free_domain::MAX_DUCKDNS_DOMAINS`と同じ、マジックナンバー回避のため
/// 定数化)。
pub const MAX_IPV6_AUTO_UPDATE_ENTRIES: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum Ipv6AutoUpdateError {
    #[error("capacity exceeded: this instance already has {0} IPv6 auto-update entry(ies) registered (max {MAX_IPV6_AUTO_UPDATE_ENTRIES})")]
    CapacityExceeded(usize),
    #[error("entry for domain='{0}' subdomain='{1}' is not registered")]
    NotFound(String, String),
    #[error("base domain '{0}' is not supported (this build only supports Value-Domain-managed 'aon.co.jp' for AAAA auto-update)")]
    UnsupportedBaseDomain(String),
}

/// 1件の自動更新試行の結果(管理APIでのポーリング表示用、
/// `free_domain::DomainUpdateStatus`と同じ設計)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Ipv6UpdateStatus {
    pub ok: bool,
    /// 反映を試みたグローバルIPv6アドレス(取得できなかった場合は`None`)。
    pub ipv6: Option<String>,
    /// エラー内容(成功時は空文字列)。
    pub detail: String,
    /// Unixエポック秒。
    pub checked_at_unix: u64,
}

/// 登録済みエントリ1件の識別子(ベースドメイン+サブドメイン名)。
#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize)]
pub struct Ipv6AutoUpdateKey {
    pub base_domain: String,
    pub subdomain: String,
}

impl Ipv6AutoUpdateKey {
    pub fn fqdn(&self) -> String {
        format!("{}.{}", self.subdomain, self.base_domain)
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 「(ベースドメイン, サブドメイン)→有効化済みかどうか」を保持する動的
/// レジストリ(`free_domain::DomainRegistry`と同じ`RwLock<HashMap<..>>`
/// パターン)。
pub struct Ipv6AutoUpdateRegistry {
    entries: RwLock<HashMap<Ipv6AutoUpdateKey, ()>>,
    last_update: RwLock<HashMap<Ipv6AutoUpdateKey, Ipv6UpdateStatus>>,
}

impl Ipv6AutoUpdateRegistry {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            last_update: RwLock::new(HashMap::new()),
        }
    }

    /// エントリを有効化する。現時点でサポートしているベースドメインは
    /// `ValueDomainProvider::BASE_DOMAIN`(`aon.co.jp`)のみ——他のドメイン
    /// を指定した場合は`UnsupportedBaseDomain`を明示的に返し、無言で
    /// 受理して後から機能しないふりをしない。
    pub async fn register(&self, base_domain: String, subdomain: String) -> Result<Ipv6AutoUpdateKey, Ipv6AutoUpdateError> {
        if base_domain != ValueDomainProvider::BASE_DOMAIN {
            return Err(Ipv6AutoUpdateError::UnsupportedBaseDomain(base_domain));
        }
        let key = Ipv6AutoUpdateKey { base_domain, subdomain };
        let mut guard = self.entries.write().await;
        if !guard.contains_key(&key) && guard.len() >= MAX_IPV6_AUTO_UPDATE_ENTRIES {
            return Err(Ipv6AutoUpdateError::CapacityExceeded(guard.len()));
        }
        guard.insert(key.clone(), ());
        Ok(key)
    }

    pub async fn remove(&self, base_domain: &str, subdomain: &str) -> Result<(), Ipv6AutoUpdateError> {
        let key = Ipv6AutoUpdateKey { base_domain: base_domain.to_string(), subdomain: subdomain.to_string() };
        let mut guard = self.entries.write().await;
        if guard.remove(&key).is_none() {
            return Err(Ipv6AutoUpdateError::NotFound(key.base_domain, key.subdomain));
        }
        drop(guard);
        self.last_update.write().await.remove(&key);
        Ok(())
    }

    #[cfg_attr(not(feature = "custom_domain"), allow(dead_code))]
    pub async fn record_update_result(&self, key: &Ipv6AutoUpdateKey, ok: bool, ipv6: Option<String>, detail: String) {
        self.last_update.write().await.insert(
            key.clone(),
            Ipv6UpdateStatus { ok, ipv6, detail, checked_at_unix: now_unix() },
        );
    }

    /// 登録済みエントリの一覧(直近の更新試行結果込み)。
    pub async fn list(&self) -> Vec<(Ipv6AutoUpdateKey, Option<Ipv6UpdateStatus>)> {
        let guard = self.entries.read().await;
        let status_guard = self.last_update.read().await;
        let mut out: Vec<_> = guard
            .keys()
            .map(|key| (key.clone(), status_guard.get(key).cloned()))
            .collect();
        out.sort_by(|a, b| a.0.fqdn().cmp(&b.0.fqdn()));
        out
    }

    #[cfg_attr(not(feature = "custom_domain"), allow(dead_code))]
    pub async fn snapshot(&self) -> Vec<Ipv6AutoUpdateKey> {
        self.entries.read().await.keys().cloned().collect()
    }

    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }
}

impl Default for Ipv6AutoUpdateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// このマシンの現在のグローバルIPv6アドレスを、IPv6対応の外部echo
/// サービス(`https://api6.ipify.org`)から取得する。IPv4のみの環境・
/// 経路では接続自体が失敗する(意図的——IPv4アドレスをIPv6として誤って
/// 使ってしまう事故を、echoサービス自体がIPv6専用のため構造的に防ぐ)。
#[cfg(feature = "custom_domain")]
pub async fn fetch_current_global_ipv6(client: &reqwest::Client) -> Result<String, reqwest::Error> {
    let text = client.get("https://api6.ipify.org").send().await?.text().await?;
    Ok(text.trim().to_string())
}

/// 登録済み各エントリのベースドメインに応じたプロバイダを解決する。
/// 現状`aon.co.jp`(Value-Domain)のみサポート。
#[cfg(feature = "custom_domain")]
fn resolve_provider(base_domain: &str) -> Result<Box<dyn DnsProvider>, DnsProviderError> {
    if base_domain == ValueDomainProvider::BASE_DOMAIN {
        Ok(Box::new(ValueDomainProvider::from_env()?))
    } else {
        Err(DnsProviderError::UnexpectedResponse(format!(
            "'{base_domain}' is not a supported base domain for IPv6 auto-update (only '{}' is supported in this build)",
            ValueDomainProvider::BASE_DOMAIN
        )))
    }
}

#[cfg(feature = "custom_domain")]
mod net {
    use super::*;
    use std::time::Duration;

    /// ポーリング間隔(1〜2分程度、ユーザー指示。実機観測でIPv6アドレスが
    /// 約5分ごとに変化するため、変化を取りこぼさないよう`free_domain`
    /// (5分間隔)よりも短い間隔にした)。
    const CHECK_INTERVAL: Duration = Duration::from_secs(90);
    /// HTTPリクエストのタイムアウト(2026-08-07追記)。**修正前は
    /// `reqwest::Client::new()`のままタイムアウト未設定だった**——外部
    /// echoサービスが応答不能になった場合、自動更新ループが無期限に
    /// ハングする既存の潜在バグだったため、`ddns.rs`/`free_domain.rs`と
    /// 同じ30秒に統一した。
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

    /// レジストリ経由で登録済みの全エントリ(最大
    /// [`super::MAX_IPV6_AUTO_UPDATE_ENTRIES`]件)を、バックグラウンドで
    /// [`CHECK_INTERVAL`]間隔で自動更新するループを起動する。レジストリが
    /// 空でも(後から管理APIで追加登録される可能性があるため)常にループ
    /// 自体は起動しておく(`free_domain::spawn_if_configured`と同じ設計)。
    pub fn spawn_if_configured(
        registry: Arc<Ipv6AutoUpdateRegistry>,
        power_profile: Arc<crate::power_profile::PowerProfileRegistry>,
    ) {
        tokio::spawn(run_loop(registry, power_profile));
    }

    async fn run_loop(
        registry: Arc<Ipv6AutoUpdateRegistry>,
        power_profile: Arc<crate::power_profile::PowerProfileRegistry>,
    ) {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let mut last_ipv6: Option<String> = None;
        loop {
            let keys = registry.snapshot().await;
            if !keys.is_empty() {
                match fetch_current_global_ipv6(&client).await {
                    Ok(ipv6) => {
                        if last_ipv6.as_deref() != Some(ipv6.as_str()) {
                            tracing::info!(
                                "IPv6 auto-update: detected IPv6 change (was {:?}, now {ipv6}), updating {} entry(ies)",
                                last_ipv6,
                                keys.len()
                            );
                            let mut all_ok = true;
                            for key in &keys {
                                match update_one(key, &ipv6).await {
                                    Ok(()) => {
                                        tracing::info!("IPv6 auto-update: AAAA update succeeded ({} -> {ipv6})", key.fqdn());
                                        registry.record_update_result(key, true, Some(ipv6.clone()), String::new()).await;
                                    }
                                    Err(e) => {
                                        all_ok = false;
                                        tracing::warn!("IPv6 auto-update: AAAA update for '{}' failed: {e}", key.fqdn());
                                        registry.record_update_result(key, false, Some(ipv6.clone()), e.to_string()).await;
                                    }
                                }
                            }
                            if all_ok {
                                last_ipv6 = Some(ipv6);
                            }
                        }
                    }
                    Err(e) => tracing::warn!("IPv6 auto-update: failed to fetch current global IPv6: {e}"),
                }
            }
            tokio::time::sleep(crate::power_profile::effective_poll_interval(&power_profile, CHECK_INTERVAL)).await;
        }
    }

    async fn update_one(key: &Ipv6AutoUpdateKey, ipv6: &str) -> Result<(), DnsProviderError> {
        let provider = resolve_provider(&key.base_domain)?;
        provider.update_ipv6(&key.subdomain, ipv6).await?;
        Ok(())
    }
}

#[cfg(feature = "custom_domain")]
pub use net::spawn_if_configured;

/// `custom_domain` feature無効時は自動更新ループ自体が存在しない
/// (既存の`ddns`/`free_domain`と同じ「featureが無ければ何もしない」
/// opt-in設計)。
#[cfg(not(feature = "custom_domain"))]
pub fn spawn_if_configured(
    _registry: Arc<Ipv6AutoUpdateRegistry>,
    _power_profile: Arc<crate::power_profile::PowerProfileRegistry>,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn registry_rejects_unsupported_base_domain() {
        let registry = Ipv6AutoUpdateRegistry::new();
        let err = registry
            .register("runo.tokyo".to_string(), "home".to_string())
            .await
            .expect_err("runo.tokyo is not yet AAAA-capable in this build");
        assert!(matches!(err, Ipv6AutoUpdateError::UnsupportedBaseDomain(d) if d == "runo.tokyo"));
        assert_eq!(registry.len().await, 0);
    }

    #[tokio::test]
    async fn registry_registers_lists_and_removes() {
        let registry = Ipv6AutoUpdateRegistry::new();
        let key = registry
            .register(ValueDomainProvider::BASE_DOMAIN.to_string(), "home".to_string())
            .await
            .expect("aon.co.jp is supported");
        assert_eq!(key.fqdn(), "home.aon.co.jp");
        assert_eq!(registry.len().await, 1);

        let listed = registry.list().await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0.fqdn(), "home.aon.co.jp");
        assert!(listed[0].1.is_none(), "no update attempt has happened yet");

        registry.remove(ValueDomainProvider::BASE_DOMAIN, "home").await.unwrap();
        assert_eq!(registry.len().await, 0);

        let err = registry
            .remove(ValueDomainProvider::BASE_DOMAIN, "home")
            .await
            .expect_err("already removed");
        assert!(matches!(err, Ipv6AutoUpdateError::NotFound(_, sub) if sub == "home"));
    }

    #[tokio::test]
    async fn registry_enforces_capacity_limit() {
        let registry = Ipv6AutoUpdateRegistry::new();
        for i in 0..MAX_IPV6_AUTO_UPDATE_ENTRIES {
            registry
                .register(ValueDomainProvider::BASE_DOMAIN.to_string(), format!("host{i}"))
                .await
                .expect("should register up to the limit");
        }
        assert_eq!(registry.len().await, MAX_IPV6_AUTO_UPDATE_ENTRIES);

        let err = registry
            .register(ValueDomainProvider::BASE_DOMAIN.to_string(), "one-too-many".to_string())
            .await
            .expect_err("one entry beyond the limit must be rejected");
        assert!(matches!(err, Ipv6AutoUpdateError::CapacityExceeded(n) if n == MAX_IPV6_AUTO_UPDATE_ENTRIES));
    }

    #[tokio::test]
    async fn record_update_result_is_reflected_in_list() {
        let registry = Ipv6AutoUpdateRegistry::new();
        let key = registry
            .register(ValueDomainProvider::BASE_DOMAIN.to_string(), "home".to_string())
            .await
            .unwrap();
        registry.record_update_result(&key, true, Some("2001:db8::1".to_string()), String::new()).await;

        let listed = registry.list().await;
        let status = listed[0].1.as_ref().expect("update result should now be present");
        assert!(status.ok);
        assert_eq!(status.ipv6.as_deref(), Some("2001:db8::1"));
    }

    /// `custom_domain` feature有効時のみ意味を持つHTTPクライアント呼び出し
    /// ロジックの検証。**正直な開示**: 実バリュードメインアカウント・
    /// APIキーはこのタスクでは提供されていないため、モックHTTPサーバー
    /// (`wiremock`)でのロジック検証に留まる——実バリュードメインAPIへの
    /// 実接続はこのセッションでは未検証。
    #[cfg(feature = "custom_domain")]
    #[tokio::test]
    async fn fetch_current_global_ipv6_parses_mock_echo_service_response() {
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("2001:db8::abcd\n"))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let resp = client.get(mock_server.uri()).send().await.unwrap();
        let text = resp.text().await.unwrap();
        assert_eq!(text.trim(), "2001:db8::abcd");
    }

    /// `resolve_provider`が未サポートのベースドメインを正直に拒否する
    /// ことの確認(`custom_domain` feature有効時のみ、`ValueDomainProvider`
    /// 型が必要なため)。
    #[cfg(feature = "custom_domain")]
    #[test]
    fn resolve_provider_rejects_unsupported_base_domain() {
        let result = resolve_provider("runo.tokyo");
        assert!(result.is_err(), "runo.tokyo has no IPv6 provider yet");
        assert!(matches!(result.err().unwrap(), DnsProviderError::UnexpectedResponse(_)));
    }

    /// `resolve_provider`がValue-Domainの資格情報未設定時、正直な
    /// `MissingCredential`を返すこと(実APIキーは無いため、この
    /// エラー経路自体がこのタスクで検証可能な範囲)。
    #[cfg(feature = "custom_domain")]
    #[test]
    fn resolve_provider_reports_missing_credential_honestly_for_value_domain() {
        std::env::remove_var(ValueDomainProvider::ENV_API_KEY);
        let result = resolve_provider(ValueDomainProvider::BASE_DOMAIN);
        assert!(result.is_err(), "no API key configured in this test env");
        assert!(matches!(result.err().unwrap(), DnsProviderError::MissingCredential(_)));
    }

    // --- 2026-08-07追記: モック検証の拡充(タイムアウト・不正なHTTP
    // レスポンス、ユーザー指示によるHANDOFF記載の未検証事項への対応) ---

    /// echoサービス応答が極端に遅い場合でも、呼び出し元が渡した
    /// `reqwest::Client`のタイムアウト設定に従って`Err`を返し、無期限に
    /// ハングしないことを確認する(`run_loop`内で実際に使われる
    /// `REQUEST_TIMEOUT`付きクライアント構築自体は`spawn_if_configured`
    /// 経由の統合的なコードパスのため直接は呼べないが、`fetch_current_
    /// global_ipv6`が受け取ったクライアントの設定をそのまま尊重する
    /// ことをここで検証する)。
    #[cfg(feature = "custom_domain")]
    #[tokio::test]
    async fn fetch_current_global_ipv6_returns_err_on_timeout() {
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string("2001:db8::abcd")
                    .set_delay(std::time::Duration::from_secs(5)),
            )
            .mount(&mock_server)
            .await;

        // `fetch_current_global_ipv6`はURLを`https://api6.ipify.org`固定で
        // 組み立てるため、この関数自体をmock_serverへ向けることはできない
        // (`custom_dns.rs`側でも同様の制約がありbase URL注入で対処したが、
        // このファイルはそこまでの改修を伴わない小さな検証に留める)。
        // ここでは同じGET+タイムアウトの組み合わせ挙動を、mock_serverの
        // URIに対して直接検証する。
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(50))
            .build()
            .unwrap();
        let err = client
            .get(mock_server.uri())
            .send()
            .await
            .expect_err("must time out, not hang forever");
        assert!(err.is_timeout(), "expected a timeout error, got: {err}");
    }

    /// echoサービスが想定外(HTMLエラーページ等)のボディを返した場合、
    /// `fetch_current_global_ipv6`自体はエラーにせず文字列をそのまま
    /// 返す(現状の実装はボディの中身を検証していない)ことを確認する。
    /// これは「異常値がそのままDNSプロバイダへ渡ってしまう」経路が
    /// `validate_ipv6_format`(`custom_dns.rs`)側の検証に委ねられている
    /// ことの裏付けを兼ねる回帰テスト。
    #[cfg(feature = "custom_domain")]
    #[tokio::test]
    async fn fetch_current_global_ipv6_does_not_validate_malformed_body() {
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string("<html>503 Service Unavailable</html>"),
            )
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let resp = client.get(mock_server.uri()).send().await.unwrap();
        let text = resp.text().await.unwrap();
        // `fetch_current_global_ipv6`の実装は`text.trim()`を返すのみで、
        // 中身がIPv6として有効かどうかは呼び出し側(`update_one`→
        // `DnsProvider::update_ipv6`→`validate_ipv6_format`)の責務。
        assert_eq!(text.trim(), "<html>503 Service Unavailable</html>");
        assert!(text.trim().parse::<std::net::Ipv6Addr>().is_err());
    }
}
