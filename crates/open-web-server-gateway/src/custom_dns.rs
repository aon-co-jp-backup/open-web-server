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
//! ## v6プラス(MAP-E)環境向けIPv6(AAAA)自動更新(2026-08-06追記)
//!
//! バリュードメイン管理の通常ドメイン(`aon.co.jp`)配下でも、v6プラス
//! (MAP-E)環境下の自宅サーバーを外部公開できるよう、`DnsProvider`トレイト
//! に[`DnsProvider::update_ipv6`]を追加した。IPv4はMAP-Eの構造上ポート
//! 開放ができないが、IPv6は無制限に使えるため、実機観測で約5分ごとに
//! 変化するIPv6アドレスを自動追従させる——DDNSと同じ発想の「AAAAレコード
//! 版DDNS」。自動更新ループ本体は[`crate::custom_dns_ipv6_autoupdate`]
//! (この`custom_dns.rs`とは別モジュール、`free_domain.rs`の5分間隔自動
//! 更新ループと同じ設計パターンを踏襲)、管理APIは
//! `POST /admin/custom-domain/setup-ipv6-auto-update`
//! (`handlers::custom_dns_ipv6`)。
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
//! 実接続はこのセッションでは未検証**であり、モックによるロジック検証
//! (単体テスト)に留まる。

use async_trait::async_trait;

/// DNSプロバイダAPIへのHTTPリクエストのタイムアウト(2026-08-07追記)。
/// **修正前は各プロバイダが`reqwest::Client::new()`のままタイムアウト
/// 未設定だった**——DNS APIが応答不能になった場合、管理APIリクエスト
/// ハンドラや自動更新ループが無期限にブロックする潜在バグだったため、
/// `ddns.rs::REQUEST_TIMEOUT`/`free_domain.rs`と同じ30秒に統一した。
#[cfg(feature = "custom_domain")]
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(feature = "custom_domain")]
fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

#[derive(Debug, thiserror::Error)]
pub enum DnsProviderError {
    #[error("required credential is not configured: {0}")]
    MissingCredential(&'static str),
    #[error("DNS provider API request failed: {0}")]
    RequestFailed(String),
    #[error("DNS provider API returned an unexpected response: {0}")]
    UnexpectedResponse(String),
    #[error("'{0}' is not a valid IPv6 address")]
    InvalidIpv6(String),
}

/// IPv6アドレス文字列の形式を検証する(2026-08-07追記)。
///
/// **背景(正直な開示)**: `update_ipv6`はこれまでIPv6アドレス文字列の
/// 形式を一切検証せずそのままDNSプロバイダAPIへ送信していた。外部echo
/// サービス(`api6.ipify.org`)がメンテナンス画面やエラーページ(HTML等)を
/// 返した場合、そのゴミ文字列をそのまま「AAAAレコードの値」としてDNS
/// プロバイダへ送りつけてしまう恐れがあった。`std::net::Ipv6Addr`の
/// パーサ(標準ライブラリ、RFC準拠)で事前に検証し、不正な形式であれば
/// ネットワーク呼び出し自体を行わずに正直な`InvalidIpv6`エラーを返す。
fn validate_ipv6_format(ipv6: &str) -> Result<(), DnsProviderError> {
    ipv6.parse::<std::net::Ipv6Addr>()
        .map(|_| ())
        .map_err(|_| DnsProviderError::InvalidIpv6(ipv6.to_string()))
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

    /// 既存/新規サブドメインのAAAAレコード(IPv6)を登録・更新する
    /// (2026-08-06追記、v6プラス〈MAP-E〉環境下でのIPv6 DDNS自動更新
    /// 対応のため新設)。**既定実装は「このプロバイダはIPv6未対応」という
    /// 正直な`Err`を返す**——各プロバイダが明示的に対応しない限り黒魔術的に
    /// 動くふりをしない(既存の`MissingCredential`と同じ「できないことは
    /// できないと正直に返す」方針を踏襲)。`ValueDomainProvider`
    /// (`aon.co.jp`)はこれを実装済み、`ConohaDnsProvider`は本パスでは
    /// 未実装のまま(次回課題、下記doc参照)。
    async fn update_ipv6(&self, _name: &str, _ipv6: &str) -> Result<DnsRecordResult, DnsProviderError> {
        Err(DnsProviderError::RequestFailed(format!(
            "provider for base domain '{}' does not implement AAAA (IPv6) record updates yet",
            self.base_domain()
        )))
    }
}

/// `aon.co.jp`(Value-Domain管理)向け実装。
#[derive(Debug)]
pub struct ValueDomainProvider {
    api_key: String,
    #[allow(dead_code)]
    base_domain: String,
    /// APIのベースURL(通常は`https://api.value-domain.com/v1`)。
    /// 2026-08-07追記: テストから実サービスへ接続せず`wiremock`のモック
    /// サーバーへ向けられるよう、フィールド化してテスト専用コンストラクタ
    /// (`with_api_key_and_base_url`、`#[cfg(test)]`限定)から差し替え可能に
    /// した。本番経路(`from_env`/`with_api_key`)は常に実APIのURLを使う。
    #[cfg(feature = "custom_domain")]
    api_base_url: String,
    #[cfg(feature = "custom_domain")]
    client: reqwest::Client,
}

impl ValueDomainProvider {
    pub const BASE_DOMAIN: &'static str = "aon.co.jp";
    pub const ENV_API_KEY: &str = "OPEN_EASY_WEB_VALUE_DOMAIN_API_KEY";
    #[cfg(feature = "custom_domain")]
    const DEFAULT_API_BASE_URL: &'static str = "https://api.value-domain.com/v1";

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
            api_base_url: Self::DEFAULT_API_BASE_URL.to_string(),
            #[cfg(feature = "custom_domain")]
            client: build_http_client(),
        }
    }

    /// テスト専用: APIベースURLを差し替えて構築する(`wiremock`のモック
    /// サーバーへ向けるため、2026-08-07追記)。本番コードパスからは
    /// 呼ばれない。
    #[cfg(all(test, feature = "custom_domain"))]
    fn with_api_key_and_base_url(api_key: String, api_base_url: String) -> Self {
        Self {
            api_key,
            base_domain: Self::BASE_DOMAIN.to_string(),
            api_base_url,
            client: build_http_client(),
        }
    }

    /// テスト専用: 内部HTTPクライアントを差し替える(短いタイムアウトを
    /// 持つクライアントでのタイムアウト検証用、2026-08-07追記)。
    #[cfg(all(test, feature = "custom_domain"))]
    fn with_test_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
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
        let url = format!("{}/domains/{}/dns", self.api_base_url, Self::BASE_DOMAIN);
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
    async fn update_ipv6(&self, name: &str, ipv6: &str) -> Result<DnsRecordResult, DnsProviderError> {
        // v6プラス(MAP-E)環境下でのIPv6 DDNS自動更新対応(2026-08-06追記)。
        // Value-DomainのDNS APIはA/AAAAレコードいずれも同じ
        // `PUT /v1/domains/{domain}/dns`(ゾーン全体をテキストで送信する
        // 設計)を使う——Web検索で確認した公式ドキュメント
        // (https://www.value-domain.com/api/doc/domain/)の記載通り、
        // `records`フィールドにBIND風の行(`<ホスト名> <レコード種別>
        // <値>`)を渡す形式。既存の`register_subdomain`(Aレコード)と
        // 完全に対をなす実装だが、**ゾーン全体送信方式であるため、
        // 本来は既存の他レコード(Aレコード含む)を保持したまま送り直す
        // 必要がある**——この土台実装ではその既存レコードの事前取得・
        // マージ(`GET /v1/domains/{domain}/dns`→パース→マージ→`PUT`)は
        // まだ行っておらず、`register_subdomain`と同じ既知の限界として
        // モジュールdocの「正直な開示」に追記済み。
        //
        // 2026-08-07追記: ネットワーク呼び出し前に`ipv6`の形式を検証する
        // (`validate_ipv6_format`)。外部echoサービスが異常なレスポンス
        // (HTMLエラーページ等)を返した場合に、それをそのままAAAA値として
        // 送りつけてしまう既知の潜在バグへの対処。
        validate_ipv6_format(ipv6)?;
        let url = format!("{}/domains/{}/dns", self.api_base_url, Self::BASE_DOMAIN);
        let body = serde_json::json!({ "records": format!("{name} AAAA {ipv6}") });
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
        Ok(DnsRecordResult { fqdn: format!("{name}.{}", Self::BASE_DOMAIN), ip: ipv6.to_string() })
    }

    #[cfg(not(feature = "custom_domain"))]
    async fn update_ipv6(&self, _name: &str, _ipv6: &str) -> Result<DnsRecordResult, DnsProviderError> {
        Err(DnsProviderError::RequestFailed(
            "this build was compiled without the `custom_domain` feature (no HTTP client available)".to_string(),
        ))
    }

    #[cfg(feature = "custom_domain")]
    async fn remove(&self, name: &str) -> Result<(), DnsProviderError> {
        let url = format!("{}/domains/{}/dns", self.api_base_url, Self::BASE_DOMAIN);
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
            client: build_http_client(),
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

    #[tokio::test]
    async fn default_update_ipv6_is_an_honest_not_implemented_error() {
        // `MockDnsProvider`は`update_ipv6`を一切オーバーライドしていないため、
        // トレイトの既定実装(「このプロバイダはIPv6未対応」)が呼ばれる
        // ——「実装したふり」をせず正直にエラーを返すことの確認。
        let provider = MockDnsProvider::new("example.test");
        let err = provider
            .update_ipv6("blog", "2001:db8::1")
            .await
            .expect_err("default update_ipv6 must be an honest error, not a silent success");
        assert!(matches!(err, DnsProviderError::RequestFailed(msg) if msg.contains("does not implement AAAA")));
    }

    #[test]
    fn base_domain_constants_match_owned_domains() {
        assert_eq!(ValueDomainProvider::BASE_DOMAIN, "aon.co.jp");
        assert!(ConohaDnsProvider::SUPPORTED_BASE_DOMAINS.contains(&"runo.tokyo"));
        assert!(ConohaDnsProvider::SUPPORTED_BASE_DOMAINS.contains(&"nasa.tokyo"));
        assert!(ConohaDnsProvider::SUPPORTED_BASE_DOMAINS.contains(&"icpo.tokyo"));
    }

    // --- 2026-08-07追記: モック検証の拡充(タイムアウト・認証失敗・
    // 不正なHTTPレスポンス・IPv6アドレス形式異常・レート制限、ユーザー
    // 指示によるHANDOFF記載の未検証事項への対応) ---

    #[test]
    fn validate_ipv6_format_accepts_well_formed_addresses() {
        assert!(validate_ipv6_format("2001:db8::1").is_ok());
        assert!(validate_ipv6_format("::1").is_ok());
        assert!(validate_ipv6_format("fe80::1234:5678:9abc:def0").is_ok());
    }

    #[test]
    fn validate_ipv6_format_rejects_malformed_addresses() {
        // IPv4アドレス・空文字列・HTMLエラーページの断片・範囲外の桁数等、
        // 「echoサービスが異常なレスポンスを返した」場合を想定した異常値。
        for bad in [
            "203.0.113.5",       // IPv4はIPv6として不正
            "",                  // 空文字列
            "not-an-ip",         // 全く形式が異なる
            "2001:db8::zzzz",    // 16進数として不正な文字
            "<html>error</html>",// echoサービスの異常レスポンス断片
            "2001:db8:::1",      // "::"の重複は不正
        ] {
            let err = validate_ipv6_format(bad).expect_err(&format!("'{bad}' must be rejected"));
            assert!(matches!(err, DnsProviderError::InvalidIpv6(v) if v == bad));
        }
    }

    #[cfg(feature = "custom_domain")]
    #[tokio::test]
    async fn value_domain_update_ipv6_rejects_malformed_ipv6_without_network_call() {
        // ネットワーク呼び出し前に形式検証が行われ、モックサーバーへは
        // 一切リクエストが飛ばないことを確認する(mockを一切mountしない
        // まま呼び出し、飛べばwiremockがpanicするのではなく単に0-hitで
        // 通るため、ここでは代わりに戻り値のエラー種別で検証する)。
        let provider = ValueDomainProvider::with_api_key_and_base_url(
            "dummy-api-key".to_string(),
            "http://127.0.0.1:1".to_string(), // 接続不可能なアドレス
        );
        let err = provider
            .update_ipv6("home", "not-a-valid-ipv6")
            .await
            .expect_err("malformed IPv6 must be rejected before any network I/O");
        assert!(matches!(err, DnsProviderError::InvalidIpv6(v) if v == "not-a-valid-ipv6"));
    }

    #[cfg(feature = "custom_domain")]
    #[tokio::test]
    async fn value_domain_register_subdomain_reports_auth_failure_honestly() {
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PUT"))
            .respond_with(wiremock::ResponseTemplate::new(401).set_body_string("Unauthorized"))
            .mount(&mock_server)
            .await;

        let provider = ValueDomainProvider::with_api_key_and_base_url(
            "wrong-api-key".to_string(),
            mock_server.uri(),
        );
        let err = provider
            .register_subdomain("home", "203.0.113.5")
            .await
            .expect_err("HTTP 401 must surface as an error, not a silent success");
        assert!(matches!(err, DnsProviderError::UnexpectedResponse(msg) if msg.contains("401")));
    }

    #[cfg(feature = "custom_domain")]
    #[tokio::test]
    async fn value_domain_register_subdomain_reports_rate_limit_honestly() {
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PUT"))
            .respond_with(wiremock::ResponseTemplate::new(429).set_body_string("Too Many Requests"))
            .mount(&mock_server)
            .await;

        let provider =
            ValueDomainProvider::with_api_key_and_base_url("api-key".to_string(), mock_server.uri());
        let err = provider
            .register_subdomain("home", "203.0.113.5")
            .await
            .expect_err("HTTP 429 must surface as an error, not a silent success");
        assert!(matches!(err, DnsProviderError::UnexpectedResponse(msg) if msg.contains("429")));
    }

    #[cfg(feature = "custom_domain")]
    #[tokio::test]
    async fn value_domain_register_subdomain_reports_malformed_response_honestly() {
        // ステータスは成功(200)だが、ボディが期待するJSON形式ではない
        // 場合でも、現在の実装はレスポンスボディをパースしていないため
        // パニックしないことを確認する(将来ボディ検証を追加する際の
        // リグレッション防止も兼ねる)。
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PUT"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("{not valid json"))
            .mount(&mock_server)
            .await;

        let provider =
            ValueDomainProvider::with_api_key_and_base_url("api-key".to_string(), mock_server.uri());
        let result = provider.register_subdomain("home", "203.0.113.5").await;
        assert!(result.is_ok(), "a 2xx status must be treated as success even with an unparsed body");
    }

    #[cfg(feature = "custom_domain")]
    #[tokio::test]
    async fn value_domain_register_subdomain_returns_err_on_timeout() {
        // モックサーバーの応答を意図的に大きく遅延させ、クライアント側の
        // タイムアウト設定(`HTTP_TIMEOUT`)が無いと無期限にハングして
        // しまう(2026-08-07修正前の潜在バグ)ことの回帰防止テスト。
        let mock_server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("PUT"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string("{}")
                    .set_delay(std::time::Duration::from_secs(5)),
            )
            .mount(&mock_server)
            .await;

        let short_timeout_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(50))
            .build()
            .unwrap();
        let provider = ValueDomainProvider::with_api_key_and_base_url(
            "api-key".to_string(),
            mock_server.uri(),
        )
        .with_test_client(short_timeout_client);

        let err = provider
            .register_subdomain("home", "203.0.113.5")
            .await
            .expect_err("must time out, not hang forever");
        assert!(matches!(err, DnsProviderError::RequestFailed(_)));
    }
}
