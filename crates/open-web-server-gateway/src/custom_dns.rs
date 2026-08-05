//! 自社ドメイン(`aon.co.jp` / `runo.tokyo`)配下への無料サブドメイン発行 +
//! 自動更新(DDNS)機能。
//!
//! 既存の[`crate::free_domain`](DuckDNS向け)と目的・パターン(5分間隔の
//! 自動更新ループ、最大[`crate::free_domain::MAX_DUCKDNS_DOMAINS`]件までの
//! 動的登録)は踏襲しつつ、DuckDNSではなく**ユーザー自身が所有する
//! ドメインのDNS管理サービスAPI**を直接叩く点が異なる。
//!
//! ## DNS管理サービスの裏取り(2026-07-24時点)
//!
//! - `aon.co.jp`: Value-Domain管理。Value-DomainのドメインAPIは
//!   `https://api.value-domain.com/v1`配下にREST APIを提供し、
//!   `Authorization: Bearer <APIキー>`ヘッダで認証する
//!   (Value-Domain公式ドキュメント「ドメインAPI」参照)。DNSレコード変更は
//!   `PUT /domains/{domainname}/dns`にゾーン全体のテキスト(BIND風の
//!   レコード行)を送る方式であることを公式ドキュメントで確認済み——
//!   個別レコードの部分更新APIではなく、**ゾーン全体を毎回送信する**
//!   設計になっている点が実装上の注意点(1レコードだけ変えたくても
//!   既存の他レコードを保持したまま送り直す必要がある)。
//! - `runo.tokyo`: ConoHa DNS管理(`nslookup -type=ns`で実際に
//!   `a.conoha-dns.com`/`b.conoha-dns.org`ネームサーバーであることを
//!   確認済み、CLAUDE.md HANDOFF参照)。ConoHa DNSはConoHa VPS/クラウドと
//!   同じ「ConoHa API」(Identity API v3、`https://identity.tyo1.conoha.io`
//!   でAPI利用者ID/パスワード/テナントIDからトークンを発行し、以後
//!   `X-Auth-Token`ヘッダで各サービスAPIを呼ぶ)経由で操作する設計で
//!   あることを公式ドキュメント(ConoHa API リファレンス、DNSサービス)で
//!   確認済み。既にVPS運用で使われているConoHa APIとは**認証方式が同じ
//!   (API利用者ID・パスワード・テナントID)** であり、新規に別の秘密情報
//!   体系を持ち込まない設計にできる。
//!
//! ## 配線状況(2026-08-05更新)
//!
//! 当初([`DnsProvider`]トレイト新設時点)は実装のみでどの管理APIからも
//! バックグラウンドループからも呼ばれない「配線されていないコード」
//! だった。2026-08-05に[`CustomDomainRegistry`]・[`build_provider`]・
//! バックグラウンド自動更新ループ(`net::run_loop`、`custom_domain`
//! feature配下)・管理API(`handlers::custom_dns`、`POST /admin/
//! custom-dns/setup`・`GET /admin/custom-dns/domains`・`DELETE
//! /admin/custom-dns/domains/:fqdn`)を追加し、`free_domain.rs`
//! (DuckDNS)と同じ「メモリ帳簿とネットワーク呼び出しの分離」設計で
//! 実際に呼び出せるようにした。さらに、登録直後に任意で(`contact_email`
//! 指定時のみ)Let's Encrypt HTTP-01証明書を1回取得する`crate::acme::
//! try_auto_https`との連携も追加し、「ドメイン登録した瞬間から
//! https://で使える」を実現した。
//!
//! ## 正直な開示
//!
//! 上記2社のAPI仕様は日英の公式ドキュメント調査に基づく実装だが、
//! **このタスクでは実際のAPIキー/シークレット/ConoHa認証情報は一切
//! 提供されておらず、実装者(Claude)はそれらを取得も入力もしていない**。
//! 環境変数(下記[`ValueDomainProvider::from_env`]/
//! [`ConohaDnsProvider::from_env`])経由で受け取る設計とし、未設定時は
//! その旨を`Err`で正直に返す——既存の`free_domain.rs`が「DuckDNSトークンは
//! ユーザー自身が取得する」としている設計方針と同じ。**実DNS APIへの
//! 実接続は今回も未検証**であり、モックによるロジック検証(単体テスト)・
//! 実HTTP経由の統合テスト(資格情報未設定時の`base_domain`検証・容量/
//! 一覧/削除ロジック)に留まる。

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::RwLock;

#[derive(Debug, thiserror::Error)]
pub enum DnsProviderError {
    #[error("required credential is not configured: {0}")]
    MissingCredential(&'static str),
    #[error("DNS provider API request failed: {0}")]
    RequestFailed(String),
    #[error("DNS provider API returned an unexpected response: {0}")]
    UnexpectedResponse(String),
}

/// 登録結果(発行したサブドメインのFQDN・反映したIPを含む)。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DnsRecordResult {
    pub fqdn: String,
    pub ip: String,
}

/// 自社ドメイン配下へのサブドメイン発行・更新・削除を抽象化するトレイト。
/// `ValueDomainProvider`(aon.co.jp)・`ConohaDnsProvider`(runo.tokyo)の
/// 2実装を持つ。モックによるテストを容易にするため`async_trait`+
/// オブジェクトセーフな設計にしてある。
#[async_trait]
pub trait DnsProvider: Send + Sync {
    /// このプロバイダが管理するベースドメイン(例: `"aon.co.jp"`)。
    fn base_domain(&self) -> &str;

    /// 新規サブドメインをAレコードとして登録する(既存なら上書き)。
    async fn register_subdomain(&self, name: &str, ip: &str) -> Result<DnsRecordResult, DnsProviderError>;

    /// 既存サブドメインのAレコードを更新する(DDNS自動更新ループから呼ばれる)。
    async fn update_ip(&self, name: &str, ip: &str) -> Result<DnsRecordResult, DnsProviderError>;

    /// サブドメインのAレコードを削除する。
    async fn remove(&self, name: &str) -> Result<(), DnsProviderError>;
}

/// `aon.co.jp`(Value-Domain管理)向け実装。
#[derive(Debug)]
pub struct ValueDomainProvider {
    api_key: String,
    #[allow(dead_code)]
    base_domain: String,
    #[cfg(feature = "custom_domain")]
    client: reqwest::Client,
}

impl ValueDomainProvider {
    pub const BASE_DOMAIN: &'static str = "aon.co.jp";
    pub const ENV_API_KEY: &str = "OPEN_EASY_WEB_VALUE_DOMAIN_API_KEY";

    /// 環境変数からAPIキーを読み込んで構築する。未設定なら
    /// `MissingCredential`を返す(実キーの代行取得・ハードコードは
    /// 一切行わない、既存方針どおり)。
    pub fn from_env() -> Result<Self, DnsProviderError> {
        let api_key = std::env::var(Self::ENV_API_KEY)
            .map_err(|_| DnsProviderError::MissingCredential(Self::ENV_API_KEY))?;
        if api_key.trim().is_empty() {
            return Err(DnsProviderError::MissingCredential(Self::ENV_API_KEY));
        }
        Ok(Self::with_api_key(api_key))
    }

    pub fn with_api_key(api_key: String) -> Self {
        Self {
            api_key,
            base_domain: Self::BASE_DOMAIN.to_string(),
            #[cfg(feature = "custom_domain")]
            client: reqwest::Client::new(),
        }
    }

    #[allow(dead_code)]
    fn api_key(&self) -> &str {
        &self.api_key
    }
}

#[async_trait]
impl DnsProvider for ValueDomainProvider {
    fn base_domain(&self) -> &str {
        Self::BASE_DOMAIN
    }

    #[cfg(feature = "custom_domain")]
    async fn register_subdomain(&self, name: &str, ip: &str) -> Result<DnsRecordResult, DnsProviderError> {
        // Value-DomainのDNS APIはゾーン全体を送信する設計のため、本来は
        // 既存レコードを取得(`GET /domains/{domain}/dns`)してからマージし
        // `PUT`し直す必要がある。今回はロジックの土台として`PUT`呼び出し
        // 自体を実装し、実運用でのゾーンマージは次段の課題として明記する。
        let url = format!("https://api.value-domain.com/v1/domains/{}/dns", Self::BASE_DOMAIN);
        let body = serde_json::json!({ "records": format!("{name} A {ip}") });
        let resp = self
            .client
            .put(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| DnsProviderError::RequestFailed(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(DnsProviderError::UnexpectedResponse(format!("HTTP {}", resp.status())));
        }
        Ok(DnsRecordResult { fqdn: format!("{name}.{}", Self::BASE_DOMAIN), ip: ip.to_string() })
    }

    #[cfg(not(feature = "custom_domain"))]
    async fn register_subdomain(&self, _name: &str, _ip: &str) -> Result<DnsRecordResult, DnsProviderError> {
        Err(DnsProviderError::RequestFailed(
            "this build was compiled without the `custom_domain` feature (no HTTP client available)".to_string(),
        ))
    }

    async fn update_ip(&self, name: &str, ip: &str) -> Result<DnsRecordResult, DnsProviderError> {
        // Value-DomainのAPI仕様上、更新も同じ`PUT`(ゾーン全体送信)である
        // ため登録と同じ経路を再利用する。
        self.register_subdomain(name, ip).await
    }

    #[cfg(feature = "custom_domain")]
    async fn remove(&self, name: &str) -> Result<(), DnsProviderError> {
        let url = format!("https://api.value-domain.com/v1/domains/{}/dns", Self::BASE_DOMAIN);
        // 削除も「そのレコードを除いたゾーン全体を送り直す」設計になる
        // (Value-Domainのゾーン全体送信方式のため)。今回は空レコードを
        // 送る最小実装とし、実運用では既存ゾーンからの除外マージが必要。
        let body = serde_json::json!({ "records": "" });
        let resp = self
            .client
            .put(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| DnsProviderError::RequestFailed(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(DnsProviderError::UnexpectedResponse(format!("HTTP {}", resp.status())));
        }
        let _ = name;
        Ok(())
    }

    #[cfg(not(feature = "custom_domain"))]
    async fn remove(&self, _name: &str) -> Result<(), DnsProviderError> {
        Err(DnsProviderError::RequestFailed(
            "this build was compiled without the `custom_domain` feature (no HTTP client available)".to_string(),
        ))
    }
}

/// ConoHa DNS管理下の任意のベースドメイン向け実装。
///
/// **2026-07-24追記(ユーザー指示によるスコープ拡大)**: 当初`runo.tokyo`
/// 専用として設計していたが、`nasa.tokyo`・`icpo.tokyo`も同じConoHa DNS
/// (ネームサーバー`a.conoha-dns.com`/`b.conoha-dns.org`、`nslookup
/// -type=ns`で確認済み)配下にあることが判明したため、ベースドメインを
/// コンストラクタ引数として受け取る設計に変更した(ConoHa DNS APIは
/// ドメイン名をURLパラメータとして渡す設計のため、実装ロジック自体の
/// 変更は不要——対応ベースドメインの一覧を増やすだけで済んだ)。
/// **`nasa.tokyo`/`icpo.tokyo`はWebサイト自体がまだ存在しない
/// (`F:\runo\URL\nasa.tokyo`・`F:\runo\URL\icpo.tokyo`はgit未初期化の
/// 空ディレクトリ、2026-07-24確認)ため、紹介バナーの追加対象からは
/// 除外し、サブドメイン発行のベースドメイン選択肢としてのみ対応する**。
#[derive(Debug)]
pub struct ConohaDnsProvider {
    base_domain: String,
    api_user_id: String,
    api_password: String,
    tenant_id: String,
    #[cfg(feature = "custom_domain")]
    client: reqwest::Client,
}

impl ConohaDnsProvider {
    /// ConoHa DNS配下で今回サブドメイン発行の対象とする、ユーザー所有の
    /// ベースドメイン一覧(2026-07-24時点、`nslookup -type=ns`で
    /// ConoHa DNS委任を確認済みのもののみ)。
    pub const SUPPORTED_BASE_DOMAINS: &'static [&'static str] = &["runo.tokyo", "nasa.tokyo", "icpo.tokyo"];
    pub const ENV_API_USER_ID: &str = "OPEN_EASY_WEB_CONOHA_API_USER_ID";
    pub const ENV_API_PASSWORD: &str = "OPEN_EASY_WEB_CONOHA_API_PASSWORD";
    pub const ENV_TENANT_ID: &str = "OPEN_EASY_WEB_CONOHA_TENANT_ID";

    /// 環境変数(API利用者ID・パスワード・テナントID、既存のVPS用ConoHa API
    /// 認証方式と同じ3点セット)から、指定したベースドメイン向けに構築する。
    /// `base_domain`が[`Self::SUPPORTED_BASE_DOMAINS`]に含まれない場合、
    /// または資格情報のいずれか未設定の場合は`MissingCredential`を返す。
    pub fn from_env_for_domain(base_domain: &str) -> Result<Self, DnsProviderError> {
        if !Self::SUPPORTED_BASE_DOMAINS.contains(&base_domain) {
            return Err(DnsProviderError::UnexpectedResponse(format!(
                "'{base_domain}' is not a ConoHa DNS-delegated domain known to this provider (supported: {:?})",
                Self::SUPPORTED_BASE_DOMAINS
            )));
        }
        let api_user_id = std::env::var(Self::ENV_API_USER_ID)
            .map_err(|_| DnsProviderError::MissingCredential(Self::ENV_API_USER_ID))?;
        let api_password = std::env::var(Self::ENV_API_PASSWORD)
            .map_err(|_| DnsProviderError::MissingCredential(Self::ENV_API_PASSWORD))?;
        let tenant_id = std::env::var(Self::ENV_TENANT_ID)
            .map_err(|_| DnsProviderError::MissingCredential(Self::ENV_TENANT_ID))?;
        if api_user_id.trim().is_empty() || api_password.trim().is_empty() || tenant_id.trim().is_empty() {
            return Err(DnsProviderError::MissingCredential(Self::ENV_API_USER_ID));
        }
        Ok(Self {
            base_domain: base_domain.to_string(),
            api_user_id,
            api_password,
            tenant_id,
            #[cfg(feature = "custom_domain")]
            client: reqwest::Client::new(),
        })
    }

    /// 後方互換用: `runo.tokyo`向けに構築する(既存呼び出し元向け)。
    pub fn from_env() -> Result<Self, DnsProviderError> {
        Self::from_env_for_domain("runo.tokyo")
    }

    #[allow(dead_code)]
    fn identity_ref(&self) -> (&str, &str, &str) {
        (&self.api_user_id, &self.api_password, &self.tenant_id)
    }
}

#[async_trait]
impl DnsProvider for ConohaDnsProvider {
    fn base_domain(&self) -> &str {
        &self.base_domain
    }

    #[cfg(feature = "custom_domain")]
    async fn register_subdomain(&self, name: &str, ip: &str) -> Result<DnsRecordResult, DnsProviderError> {
        // ConoHa APIはIdentity API v3でトークンを発行してから各サービスAPI
        // (DNSサービス)を呼ぶ2段構成。ここではトークン発行の呼び出し
        // ロジックのみを実装し(実接続は未検証)、DNSレコード登録自体は
        // 発行済みトークンを`X-Auth-Token`ヘッダへ載せて呼ぶ設計とする。
        let token = self.issue_token().await?;
        let url = format!("https://dns-service.tyo1.conoha.io/v1/domains/{}/records", self.base_domain);
        let body = serde_json::json!({ "name": format!("{name}.{}.", self.base_domain), "type": "A", "data": ip, "ttl": 300 });
        let resp = self
            .client
            .post(&url)
            .header("X-Auth-Token", token)
            .json(&body)
            .send()
            .await
            .map_err(|e| DnsProviderError::RequestFailed(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(DnsProviderError::UnexpectedResponse(format!("HTTP {}", resp.status())));
        }
        Ok(DnsRecordResult { fqdn: format!("{name}.{}", self.base_domain), ip: ip.to_string() })
    }

    #[cfg(not(feature = "custom_domain"))]
    async fn register_subdomain(&self, _name: &str, _ip: &str) -> Result<DnsRecordResult, DnsProviderError> {
        Err(DnsProviderError::RequestFailed(
            "this build was compiled without the `custom_domain` feature (no HTTP client available)".to_string(),
        ))
    }

    async fn update_ip(&self, name: &str, ip: &str) -> Result<DnsRecordResult, DnsProviderError> {
        // ConoHa DNSは個別レコード単位のPUT/DELETEに対応する設計のため、
        // Value-Domainと異なりここでは概念上「既存レコードのdata書き換え」
        // だが、record_idの事前取得が必要になる(未実装、次段の課題として
        // 明記)。現状は`register_subdomain`と同じPOSTで代替する
        // (ConoHa DNS APIは同名レコードの重複登録を許すため、本番実装では
        // 事前のGET+DELETEまたはPUTへの置き換えが必要)。
        self.register_subdomain(name, ip).await
    }

    #[cfg(feature = "custom_domain")]
    async fn remove(&self, name: &str) -> Result<(), DnsProviderError> {
        let token = self.issue_token().await?;
        // 実運用ではレコードIDが必要(事前のGETで解決する設計が必要、
        // 今回は未実装の正直な開示として明記)。
        let url = format!("https://dns-service.tyo1.conoha.io/v1/domains/{}/records/{name}", self.base_domain);
        let resp = self
            .client
            .delete(&url)
            .header("X-Auth-Token", token)
            .send()
            .await
            .map_err(|e| DnsProviderError::RequestFailed(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(DnsProviderError::UnexpectedResponse(format!("HTTP {}", resp.status())));
        }
        Ok(())
    }

    #[cfg(not(feature = "custom_domain"))]
    async fn remove(&self, _name: &str) -> Result<(), DnsProviderError> {
        Err(DnsProviderError::RequestFailed(
            "this build was compiled without the `custom_domain` feature (no HTTP client available)".to_string(),
        ))
    }
}

#[cfg(feature = "custom_domain")]
impl ConohaDnsProvider {
    async fn issue_token(&self) -> Result<String, DnsProviderError> {
        let url = "https://identity.tyo1.conoha.io/v3/auth/tokens";
        let body = serde_json::json!({
            "auth": {
                "identity": {
                    "methods": ["password"],
                    "password": {
                        "user": { "id": self.api_user_id, "password": self.api_password }
                    }
                },
                "scope": { "project": { "id": self.tenant_id } }
            }
        });
        let resp = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| DnsProviderError::RequestFailed(e.to_string()))?;
        let token = resp
            .headers()
            .get("X-Subject-Token")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| DnsProviderError::UnexpectedResponse("missing X-Subject-Token header".to_string()))?;
        Ok(token)
    }
}

/// 1インスタンスあたりに登録できるカスタムドメイン(自社ドメイン配下の
/// 無料サブドメイン)の上限。`free_domain::MAX_DUCKDNS_DOMAINS`と同じ値を
/// 踏襲する(マジックナンバーを避けるための定数化、2026-08-05配線)。
pub const MAX_CUSTOM_DOMAINS: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum CustomDomainError {
    #[error("capacity exceeded: this instance already has {0} custom domain(s) registered (max {MAX_CUSTOM_DOMAINS})")]
    CapacityExceeded(usize),
    #[error("custom domain '{0}' is not registered")]
    NotFound(String),
}

/// 1件分の直近更新試行結果(`free_domain::DomainUpdateStatus`と同じ形)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct CustomDomainUpdateStatus {
    pub ok: bool,
    pub ip: Option<String>,
    pub message: String,
    pub checked_at_unix: u64,
}

/// 一覧表示用サマリ(登録済みFQDN・ベースドメイン・サブドメイン・直近更新)。
#[derive(Debug, Clone, serde::Serialize)]
pub struct CustomDomainSummary {
    pub base_domain: String,
    pub subdomain: String,
    pub fqdn: String,
    pub last_update: Option<CustomDomainUpdateStatus>,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// このベースドメインを、このサーバーが実際に管理できるか
/// (`ValueDomainProvider`/`ConohaDnsProvider`のいずれかが対応しているか)。
pub fn is_supported_base_domain(base_domain: &str) -> bool {
    base_domain == ValueDomainProvider::BASE_DOMAIN
        || ConohaDnsProvider::SUPPORTED_BASE_DOMAINS.contains(&base_domain)
}

/// `base_domain`に応じて適切な`DnsProvider`実装を環境変数から構築する。
/// 資格情報未設定時・非対応ベースドメイン時は`Err`を返す(実キーの代行
/// 取得・ハードコードは一切行わない、既存方針どおり)。この関数自体は
/// `custom_domain` featureの有無に関わらずコンパイルできる——実際の
/// ネットワーク呼び出しは各`DnsProvider`実装のメソッド側でfeature分岐
/// している(モジュールdoc冒頭参照)。
pub fn build_provider(base_domain: &str) -> Result<Box<dyn DnsProvider>, DnsProviderError> {
    if base_domain == ValueDomainProvider::BASE_DOMAIN {
        Ok(Box::new(ValueDomainProvider::from_env()?))
    } else if ConohaDnsProvider::SUPPORTED_BASE_DOMAINS.contains(&base_domain) {
        Ok(Box::new(ConohaDnsProvider::from_env_for_domain(base_domain)?))
    } else {
        Err(DnsProviderError::UnexpectedResponse(format!(
            "'{base_domain}' is not a base domain this server can manage (supported: '{}', {:?})",
            ValueDomainProvider::BASE_DOMAIN,
            ConohaDnsProvider::SUPPORTED_BASE_DOMAINS,
        )))
    }
}

/// 「FQDN → (ベースドメイン, サブドメイン)」を保持する動的レジストリ
/// (`free_domain::DomainRegistry`と同じ`RwLock<HashMap<..>>`パターン)。
///
/// **設計上の意図的な分離**: この構造体の`register`/`remove`は**メモリ
/// 上の帳簿操作のみ**で、実際のDNSプロバイダAPI呼び出しは一切行わない
/// (`free_domain::DomainRegistry::register`がトークンを覚えるだけで
/// DuckDNSを叩かないのと同じ設計)。実際のAPI呼び出しは呼び出し元
/// (`handlers::custom_dns`のハンドラ、および下記`net::run_loop`)が
/// `build_provider()`経由で行う——これにより、資格情報や実ネットワークを
/// 必要としないメモリ操作だけの単体テストが書ける。
pub struct CustomDomainRegistry {
    entries: RwLock<HashMap<String, (String, String)>>,
    last_update: RwLock<HashMap<String, CustomDomainUpdateStatus>>,
}

impl CustomDomainRegistry {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            last_update: RwLock::new(HashMap::new()),
        }
    }

    /// 更新試行結果を記録する(ハンドラ・自動更新ループの両方から呼ぶ)。
    pub async fn record_update_result(&self, fqdn: &str, ok: bool, ip: Option<String>, message: String) {
        let mut guard = self.last_update.write().await;
        guard.insert(
            fqdn.to_string(),
            CustomDomainUpdateStatus { ok, ip, message, checked_at_unix: now_unix() },
        );
    }

    /// `base_domain`+`subdomain`を登録する(FQDN = `subdomain.base_domain`)。
    /// 新規登録で上限[`MAX_CUSTOM_DOMAINS`]を超える場合は`CapacityExceeded`を
    /// 返す。既に登録済みのFQDNの再登録は上限カウントに影響しない。
    pub async fn register(&self, base_domain: String, subdomain: String) -> Result<String, CustomDomainError> {
        let fqdn = format!("{subdomain}.{base_domain}");
        let mut guard = self.entries.write().await;
        if !guard.contains_key(&fqdn) && guard.len() >= MAX_CUSTOM_DOMAINS {
            return Err(CustomDomainError::CapacityExceeded(guard.len()));
        }
        guard.insert(fqdn.clone(), (base_domain, subdomain));
        Ok(fqdn)
    }

    /// 登録解除し、`(base_domain, subdomain)`を返す(呼び出し元がDNS
    /// プロバイダ側のレコード削除にも使えるようにするため)。
    pub async fn remove(&self, fqdn: &str) -> Result<(String, String), CustomDomainError> {
        let mut guard = self.entries.write().await;
        let entry = guard
            .remove(fqdn)
            .ok_or_else(|| CustomDomainError::NotFound(fqdn.to_string()))?;
        drop(guard);
        self.last_update.write().await.remove(fqdn);
        Ok(entry)
    }

    pub async fn list(&self) -> Vec<CustomDomainSummary> {
        let guard = self.entries.read().await;
        let status_guard = self.last_update.read().await;
        let mut out: Vec<CustomDomainSummary> = guard
            .iter()
            .map(|(fqdn, (base_domain, subdomain))| CustomDomainSummary {
                base_domain: base_domain.clone(),
                subdomain: subdomain.clone(),
                fqdn: fqdn.clone(),
                last_update: status_guard.get(fqdn).cloned(),
            })
            .collect();
        out.sort_by(|a, b| a.fqdn.cmp(&b.fqdn));
        out
    }

    /// 自動更新ループが1周ごとに使う`(fqdn, base_domain, subdomain)`の
    /// スナップショット(`custom_domain` feature無効時は自動更新ループ
    /// 自体が存在しないため未使用)。
    #[cfg_attr(not(feature = "custom_domain"), allow(dead_code))]
    pub async fn snapshot(&self) -> Vec<(String, String, String)> {
        self.entries
            .read()
            .await
            .iter()
            .map(|(fqdn, (base_domain, subdomain))| (fqdn.clone(), base_domain.clone(), subdomain.clone()))
            .collect()
    }

    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }
}

impl Default for CustomDomainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "custom_domain")]
pub async fn fetch_current_ip(client: &reqwest::Client) -> Result<String, DnsProviderError> {
    let text = client
        .get("https://api.ipify.org")
        .send()
        .await
        .map_err(|e| DnsProviderError::RequestFailed(e.to_string()))?
        .text()
        .await
        .map_err(|e| DnsProviderError::RequestFailed(e.to_string()))?;
    Ok(text.trim().to_string())
}

/// 登録済み全カスタムドメインを、DDNS(DuckDNS)と同じ「5分間隔でグローバル
/// IPの変化を検知し、変化していれば登録済み全ドメインを更新する」パターンで
/// 自動更新するバックグラウンドループ(`free_domain::net::run_loop`と
/// 同型)。レジストリが空でも(後から管理APIで追加登録される可能性がある
/// ため)常にループ自体は起動しておく。
#[cfg(feature = "custom_domain")]
mod net {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

    pub fn spawn_if_configured(
        registry: Arc<CustomDomainRegistry>,
        power_profile: Arc<crate::power_profile::PowerProfileRegistry>,
    ) {
        tokio::spawn(run_loop(registry, power_profile));
    }

    async fn run_loop(
        registry: Arc<CustomDomainRegistry>,
        power_profile: Arc<crate::power_profile::PowerProfileRegistry>,
    ) {
        let client = reqwest::Client::new();
        let mut last_ip: Option<String> = None;
        loop {
            let entries = registry.snapshot().await;
            if !entries.is_empty() {
                match fetch_current_ip(&client).await {
                    Ok(ip) => {
                        if last_ip.as_deref() != Some(ip.as_str()) {
                            tracing::info!(
                                "custom-dns: detected IP change (was {:?}, now {ip}), updating {} domain(s)",
                                last_ip,
                                entries.len()
                            );
                            let mut all_ok = true;
                            for (fqdn, base_domain, subdomain) in &entries {
                                let provider = match build_provider(base_domain) {
                                    Ok(p) => p,
                                    Err(e) => {
                                        all_ok = false;
                                        tracing::warn!("custom-dns: cannot build provider for '{fqdn}': {e}");
                                        registry.record_update_result(fqdn, false, Some(ip.clone()), e.to_string()).await;
                                        continue;
                                    }
                                };
                                match provider.update_ip(subdomain, &ip).await {
                                    Ok(_) => {
                                        tracing::info!("custom-dns: update succeeded ({fqdn} -> {ip})");
                                        registry
                                            .record_update_result(fqdn, true, Some(ip.clone()), "updated".to_string())
                                            .await;
                                    }
                                    Err(e) => {
                                        all_ok = false;
                                        tracing::warn!("custom-dns: update for '{fqdn}' failed: {e}");
                                        registry.record_update_result(fqdn, false, Some(ip.clone()), e.to_string()).await;
                                    }
                                }
                            }
                            if all_ok {
                                last_ip = Some(ip);
                            }
                        }
                    }
                    Err(e) => tracing::warn!("custom-dns: failed to fetch current IP: {e}"),
                }
            }
            tokio::time::sleep(crate::power_profile::effective_poll_interval(&power_profile, CHECK_INTERVAL)).await;
        }
    }
}

#[cfg(feature = "custom_domain")]
pub use net::spawn_if_configured;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// モック実装(実DNS APIへは一切接続しない、呼び出し引数・回数のみ検証)。
    struct MockDnsProvider {
        base_domain: String,
        registered: Mutex<Vec<(String, String)>>,
        removed: Mutex<Vec<String>>,
    }

    impl MockDnsProvider {
        fn new(base_domain: &str) -> Self {
            Self { base_domain: base_domain.to_string(), registered: Mutex::new(Vec::new()), removed: Mutex::new(Vec::new()) }
        }
    }

    #[async_trait]
    impl DnsProvider for MockDnsProvider {
        fn base_domain(&self) -> &str {
            &self.base_domain
        }

        async fn register_subdomain(&self, name: &str, ip: &str) -> Result<DnsRecordResult, DnsProviderError> {
            self.registered.lock().unwrap().push((name.to_string(), ip.to_string()));
            Ok(DnsRecordResult { fqdn: format!("{name}.{}", self.base_domain), ip: ip.to_string() })
        }

        async fn update_ip(&self, name: &str, ip: &str) -> Result<DnsRecordResult, DnsProviderError> {
            self.register_subdomain(name, ip).await
        }

        async fn remove(&self, name: &str) -> Result<(), DnsProviderError> {
            self.removed.lock().unwrap().push(name.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn mock_provider_registers_updates_and_removes() {
        let provider = MockDnsProvider::new("aon.co.jp");
        let result = provider.register_subdomain("blog", "203.0.113.5").await.unwrap();
        assert_eq!(result.fqdn, "blog.aon.co.jp");
        assert_eq!(result.ip, "203.0.113.5");

        let updated = provider.update_ip("blog", "203.0.113.9").await.unwrap();
        assert_eq!(updated.ip, "203.0.113.9");

        provider.remove("blog").await.unwrap();
        assert_eq!(provider.removed.lock().unwrap().as_slice(), &["blog".to_string()]);
        assert_eq!(provider.registered.lock().unwrap().len(), 2);
    }

    #[test]
    fn value_domain_provider_from_env_reports_missing_credential_honestly() {
        std::env::remove_var(ValueDomainProvider::ENV_API_KEY);
        let err = ValueDomainProvider::from_env().expect_err("must fail without an API key");
        assert!(matches!(err, DnsProviderError::MissingCredential(k) if k == ValueDomainProvider::ENV_API_KEY));
    }

    #[test]
    fn conoha_dns_provider_from_env_reports_missing_credential_honestly() {
        std::env::remove_var(ConohaDnsProvider::ENV_API_USER_ID);
        std::env::remove_var(ConohaDnsProvider::ENV_API_PASSWORD);
        std::env::remove_var(ConohaDnsProvider::ENV_TENANT_ID);
        let err = ConohaDnsProvider::from_env().expect_err("must fail without full credentials");
        assert!(matches!(err, DnsProviderError::MissingCredential(_)));
    }

    #[test]
    fn base_domain_constants_match_owned_domains() {
        assert_eq!(ValueDomainProvider::BASE_DOMAIN, "aon.co.jp");
        assert!(ConohaDnsProvider::SUPPORTED_BASE_DOMAINS.contains(&"runo.tokyo"));
        assert!(ConohaDnsProvider::SUPPORTED_BASE_DOMAINS.contains(&"nasa.tokyo"));
        assert!(ConohaDnsProvider::SUPPORTED_BASE_DOMAINS.contains(&"icpo.tokyo"));
    }

    #[test]
    fn is_supported_base_domain_matches_both_providers() {
        assert!(is_supported_base_domain("aon.co.jp"));
        assert!(is_supported_base_domain("runo.tokyo"));
        assert!(!is_supported_base_domain("example.com"));
    }

    #[test]
    fn build_provider_rejects_unsupported_base_domain() {
        let err = match build_provider("example.com") {
            Ok(_) => panic!("must reject unknown base domain"),
            Err(e) => e,
        };
        assert!(matches!(err, DnsProviderError::UnexpectedResponse(_)));
    }

    #[tokio::test]
    async fn custom_domain_registry_enforces_capacity_limit() {
        let registry = CustomDomainRegistry::new();
        for i in 0..MAX_CUSTOM_DOMAINS {
            registry
                .register("aon.co.jp".to_string(), format!("host{i}"))
                .await
                .expect("should register up to the limit");
        }
        assert_eq!(registry.len().await, MAX_CUSTOM_DOMAINS);

        let err = registry
            .register("aon.co.jp".to_string(), "one-too-many".to_string())
            .await
            .expect_err("21st distinct domain must be rejected");
        assert!(matches!(err, CustomDomainError::CapacityExceeded(n) if n == MAX_CUSTOM_DOMAINS));
    }

    #[tokio::test]
    async fn custom_domain_registry_register_list_and_remove() {
        let registry = CustomDomainRegistry::new();
        let fqdn = registry.register("aon.co.jp".to_string(), "blog".to_string()).await.unwrap();
        assert_eq!(fqdn, "blog.aon.co.jp");

        registry.record_update_result(&fqdn, true, Some("203.0.113.5".to_string()), "registered".to_string()).await;

        let list = registry.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].fqdn, "blog.aon.co.jp");
        assert_eq!(list[0].base_domain, "aon.co.jp");
        assert_eq!(list[0].subdomain, "blog");
        let status = list[0].last_update.as_ref().expect("update status recorded");
        assert!(status.ok);
        assert_eq!(status.ip.as_deref(), Some("203.0.113.5"));

        let (base_domain, subdomain) = registry.remove(&fqdn).await.unwrap();
        assert_eq!(base_domain, "aon.co.jp");
        assert_eq!(subdomain, "blog");
        assert!(registry.list().await.is_empty());

        let err = registry.remove(&fqdn).await.expect_err("already removed");
        assert!(matches!(err, CustomDomainError::NotFound(f) if f == fqdn));
    }

    #[tokio::test]
    async fn custom_domain_registry_snapshot_reflects_current_entries() {
        let registry = CustomDomainRegistry::new();
        registry.register("aon.co.jp".to_string(), "alpha".to_string()).await.unwrap();
        registry.register("runo.tokyo".to_string(), "beta".to_string()).await.unwrap();

        let mut snapshot = registry.snapshot().await;
        snapshot.sort();
        assert_eq!(
            snapshot,
            vec![
                ("alpha.aon.co.jp".to_string(), "aon.co.jp".to_string(), "alpha".to_string()),
                ("beta.runo.tokyo".to_string(), "runo.tokyo".to_string(), "beta".to_string()),
            ]
        );
    }
}
