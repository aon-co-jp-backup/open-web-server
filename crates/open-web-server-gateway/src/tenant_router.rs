//! マルチテナント・ドメインルーター(open-easyweb構想 第一実装)。
//!
//! 従来の「ドメイン/サブドメインごとに Web サーバー・バックエンド
//! (open-runo / poem-cosmo-tauri)・DB を個別インストール」という運用を、
//! **1プロセス内でホストヘッダに応じて動的にバックエンドへ振り分ける**
//! 設計に置き換える。
//!
//! - ドメイン追加/削除は `TenantRegistry` への追加/削除のみで完結し、
//!   プロセス再起動やポート個別割り当ては不要(ノーダウンタイム)。
//! - `tokio::sync::RwLock<HashMap<..>>` による共有レジストリなので、
//!   ドメインごとに OS スレッド/プロセスを増やさず、hyper が受けた
//!   接続はマルチコアの tokio ランタイム上で自然に分散される
//!   (「分身の術」= プロセスの複製ではなく、軽量な非同期タスク単位の複製)。
//! - 設定は宣言的な TOML 1本(`domains.toml`)から一括ロードでき、
//!   個別インストール手順を無くすことを狙う。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// `domains.toml` の直列化用ラッパー(`load_from_toml`の読み込み形式と対称)。
#[derive(Serialize, Deserialize)]
struct DomainsFile {
    #[serde(rename = "domain", default)]
    domains: Vec<TenantConfig>,
}

/// このドメインが使うバックエンドの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    OpenRuno,
    PoemCosmoTauri,
}

/// 1ドメイン分の設定(宣言的プロビジョニングの単位)。
///
/// `domains.toml` の `[[domain]]` テーブル、または管理API
/// `POST /admin/tenants` のJSONボディがこの構造にデシリアライズされる。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantConfig {
    /// 振り分け対象のHostヘッダ値(例: "shop.example.com")。
    pub host: String,
    pub backend: BackendKind,
    /// バックエンド(open-runo / poem-cosmo-tauri)へのリバースプロキシ先。
    pub backend_addr: String,
    /// このテナントが使う DB (aruaru-db 等)への接続文字列。
    pub db_uri: String,
    /// 任意のパスプレフィックス(例: "/blog")。指定された場合、同じHostの
    /// 配下で「このプレフィックスから始まるパスのみ」をこのテナントへ
    /// 振り分ける("分身の術"の対象拡大、2026-07-22追記)。省略時(`None`)は
    /// 従来通りHostのみでマッチし、後方互換を維持する(既存の登録済み
    /// ルールは一切壊れない)。バックエンドへ転送する際はこのプレフィックス
    /// 部分をリクエストパスから除去してから渡す(RS-Blog/RS-Chiketto/RS-EC
    /// はいずれも`/`をトップとして期待するルーティング実装のため)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
}

/// ルーティング済みテナントの実行時ハンドル。
#[derive(Debug, Clone)]
pub struct TenantHandle {
    pub config: TenantConfig,
}

/// 全ドメインの登録・検索を担う共有レジストリ。
///
/// `Arc<TenantRegistry>` として `AppState` に載せ、リクエスト処理と
/// 管理API(追加/削除)の両方から参照する。書き込みは追加/削除時のみで、
/// 通常のリクエスト処理は読み取りロックのみなので、ドメイン数が増えても
/// リクエストパスの競合は増えない。
#[derive(Debug, Default)]
pub struct TenantRegistry {
    /// Hostごとに1件以上のテナントを保持する(同一Host配下で複数の
    /// `path_prefix`を持つテナントを共存させるため、2026-07-22に
    /// `Arc<TenantHandle>`単体から`Vec`へ変更。`path_prefix`未指定の
    /// テナントは通常1Host1件のままで、この変更による既存動作への
    /// 影響は無い)。
    tenants: RwLock<HashMap<String, Vec<Arc<TenantHandle>>>>,
    /// 設定済みの場合、`add`/`remove`/`upsert`のたびに現在の全テナントを
    /// このパスへTOMLとして書き戻す(Apacheの`a2ensite`相当の永続化。
    /// 管理APIでの変更がプロセス再起動後も残るようにするための実用性
    /// 向上——従来はメモリ内のみで、再起動すると消えていた)。
    persist_path: RwLock<Option<PathBuf>>,
}

#[derive(Debug, thiserror::Error)]
pub enum TenantError {
    #[error("host '{0}' is not registered")]
    NotFound(String),
    #[error("host '{0}' is already registered")]
    AlreadyExists(String),
}

impl TenantRegistry {
    pub fn new() -> Self {
        Self {
            tenants: RwLock::new(HashMap::new()),
            persist_path: RwLock::new(None),
        }
    }

    /// 以後の`add`/`remove`/`upsert`を、指定パスのTOMLファイルへ自動的に
    /// 書き戻すようにする(`OPEN_WEB_SERVER_DOMAINS_FILE`起動時ロードと
    /// 対にして使う想定)。
    pub async fn set_persist_path(&self, path: PathBuf) {
        *self.persist_path.write().await = Some(path);
    }

    /// 現在のテナント一覧を、設定済みの永続化パスへ原子的に(一時ファイル
    /// →rename)書き戻す。パス未設定なら何もしない。書き込み失敗は
    /// 呼び出し元のadd/remove自体を失敗させない(リクエストパスの
    /// 可用性を優先し、警告ログのみ残す)。
    async fn persist(&self, tenants: &HashMap<String, Vec<Arc<TenantHandle>>>) {
        let Some(path) = self.persist_path.read().await.clone() else {
            return;
        };

        let domains: Vec<TenantConfig> = tenants
            .values()
            .flatten()
            .map(|h| h.config.clone())
            .collect();
        let file = DomainsFile { domains };

        let toml_str = match toml::to_string_pretty(&file) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "failed to serialize domains.toml for persistence");
                return;
            }
        };

        let tmp_path = path.with_extension("toml.tmp");
        if let Err(e) = tokio::fs::write(&tmp_path, toml_str).await {
            tracing::warn!(error = %e, path = %tmp_path.display(), "failed to write domains.toml tmp file");
            return;
        }
        if let Err(e) = tokio::fs::rename(&tmp_path, &path).await {
            tracing::warn!(error = %e, path = %path.display(), "failed to persist domains.toml (rename)");
        }
    }

    /// `domains.toml` 相当のTOML文字列から一括ロードする。
    ///
    /// 個別インストール作業を「設定ファイル1本」に集約するための入口。
    pub async fn load_from_toml(&self, toml_str: &str) -> anyhow::Result<usize> {
        #[derive(Deserialize)]
        struct DomainsFile {
            #[serde(rename = "domain", default)]
            domains: Vec<TenantConfig>,
        }

        let parsed: DomainsFile = toml::from_str(toml_str)?;
        let mut guard = self.tenants.write().await;
        let count = parsed.domains.len();
        for config in parsed.domains {
            guard
                .entry(config.host.clone())
                .or_default()
                .push(Arc::new(TenantHandle { config }));
        }
        Ok(count)
    }

    /// ドメインを1件追加する(ノーダウンタイム、既存接続には影響しない)。
    /// 指定ホスト(サブドメイン含むフルホスト名)が既に登録済みか
    /// (`path_prefix`を問わず、そのホストに何か1件でも登録があるか)。
    /// `update_tenant`ハンドラで「変更」と「新規追加」を区別するために使う。
    pub async fn exists(&self, host: &str) -> bool {
        self.tenants
            .read()
            .await
            .get(host)
            .is_some_and(|v| !v.is_empty())
    }

    /// 同一Host配下で(path_prefixの有無・値まで含めて)完全に同じ組み合わせが
    /// 既に登録されていないかを見る(重複登録防止、`path_prefix`が異なれば
    /// 別テナントとして共存できる——"分身の術"の対象拡大)。
    pub async fn add(&self, config: TenantConfig) -> Result<(), TenantError> {
        let mut guard = self.tenants.write().await;
        let entries = guard.entry(config.host.clone()).or_default();
        if entries
            .iter()
            .any(|h| h.config.path_prefix == config.path_prefix)
        {
            return Err(TenantError::AlreadyExists(config.host));
        }
        entries.push(Arc::new(TenantHandle { config }));
        self.persist(&guard).await;
        Ok(())
    }

    /// 既存ドメイン(+`path_prefix`の組み合わせ)の設定を置き換える
    /// (存在しなければ追加と同じ扱い)。
    pub async fn upsert(&self, config: TenantConfig) {
        let mut guard = self.tenants.write().await;
        let entries = guard.entry(config.host.clone()).or_default();
        entries.retain(|h| h.config.path_prefix != config.path_prefix);
        entries.push(Arc::new(TenantHandle { config }));
        self.persist(&guard).await;
    }

    /// ドメインを削除する。**後方互換の挙動**: 引数は従来通りHostのみ
    /// (`path_prefix`を持たない=省略された、旧来の1Host1テナント運用を
    /// 想定した登録)を対象に削除する。`path_prefix`付きテナントを個別に
    /// 削除したい場合は`remove_prefixed`を使う。
    pub async fn remove(&self, host: &str) -> Result<(), TenantError> {
        self.remove_prefixed(host, None).await
    }

    /// `host` + `path_prefix`の組み合わせでテナントを1件削除する。
    pub async fn remove_prefixed(
        &self,
        host: &str,
        path_prefix: Option<&str>,
    ) -> Result<(), TenantError> {
        let mut guard = self.tenants.write().await;
        let Some(entries) = guard.get_mut(host) else {
            return Err(TenantError::NotFound(host.to_string()));
        };
        let before = entries.len();
        entries.retain(|h| h.config.path_prefix.as_deref() != path_prefix);
        if entries.len() == before {
            return Err(TenantError::NotFound(host.to_string()));
        }
        if entries.is_empty() {
            guard.remove(host);
        }
        self.persist(&guard).await;
        Ok(())
    }

    /// Hostヘッダ(ポート番号が付いている場合は除去して) + リクエストパスから
    /// テナントを引く。マッチ規則(2026-07-22、"分身の術"の対象拡大):
    /// 1. そのHost配下で`path_prefix`が指定されておりリクエストパスが
    ///    そのプレフィックスで始まる登録のうち、最も長いプレフィックスの
    ///    ものを優先する(`/blog`と`/blog/api`のように複数該当し得る場合、
    ///    より具体的な方を選ぶ)。
    /// 2. 該当が無ければ、`path_prefix`未指定(=Hostのみでマッチする従来
    ///    どおりの登録)のテナントを返す。
    /// 3. どちらも無ければ`None`。
    pub async fn resolve(&self, host_header: &str, path: &str) -> Option<Arc<TenantHandle>> {
        self.resolve_prefix_only(host_header, path)
            .await
            .or(self.resolve_host_only(host_header).await)
    }

    /// `resolve`のうち、`path_prefix`が実際に指定・一致した場合のみを返す
    /// (host-onlyテナントへのフォールバックはしない)。`web_vhost`のような
    /// 「hostのみでマッチする既存の仕組み」より本当に優先すべきなのは
    /// パスプレフィックスが明示的に一致した場合だけであり、host-only
    /// フォールバックまで含めてしまうと`web_vhost`より常に先に評価される
    /// 既存の優先順位を崩してしまうため、`main::dispatch()`側で使い分ける
    /// 目的で公開する(2026-07-22追記)。
    pub async fn resolve_prefix_only(
        &self,
        host_header: &str,
        path: &str,
    ) -> Option<Arc<TenantHandle>> {
        let host = host_header.split(':').next().unwrap_or(host_header);
        let guard = self.tenants.read().await;
        let entries = guard.get(host)?;

        entries
            .iter()
            .filter_map(|h| {
                h.config
                    .path_prefix
                    .as_ref()
                    .filter(|p| !p.is_empty() && path.starts_with(p.as_str()))
                    .map(|p| (p.len(), h))
            })
            .max_by_key(|(len, _)| *len)
            .map(|(_, h)| h.clone())
    }

    /// `path_prefix`未指定(従来通りHostのみでマッチする)テナントのみを返す。
    pub async fn resolve_host_only(&self, host_header: &str) -> Option<Arc<TenantHandle>> {
        let host = host_header.split(':').next().unwrap_or(host_header);
        let guard = self.tenants.read().await;
        let entries = guard.get(host)?;
        entries
            .iter()
            .find(|h| h.config.path_prefix.is_none())
            .cloned()
    }

    pub async fn list(&self) -> Vec<TenantConfig> {
        self.tenants
            .read()
            .await
            .values()
            .flatten()
            .map(|h| h.config.clone())
            .collect()
    }

    pub async fn len(&self) -> usize {
        self.tenants.read().await.values().map(|v| v.len()).sum()
    }

    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

/// リクエストパス先頭から`path_prefix`を除去したパスを返す(バックエンドは
/// `/`をトップとして期待するため)。除去後に空文字列になる場合は`"/"`を返す。
pub fn strip_path_prefix(path: &str, prefix: &str) -> String {
    let stripped = path.strip_prefix(prefix).unwrap_or(path);
    if stripped.is_empty() {
        "/".to_string()
    } else if stripped.starts_with('/') {
        stripped.to_string()
    } else {
        format!("/{stripped}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(host: &str) -> TenantConfig {
        TenantConfig {
            host: host.to_string(),
            backend: BackendKind::OpenRuno,
            backend_addr: "127.0.0.1:9001".to_string(),
            db_uri: "postgres://localhost/db".to_string(),
            path_prefix: None,
        }
    }

    fn sample_prefixed(host: &str, prefix: &str, backend_addr: &str) -> TenantConfig {
        TenantConfig {
            host: host.to_string(),
            backend: BackendKind::OpenRuno,
            backend_addr: backend_addr.to_string(),
            db_uri: "postgres://localhost/db".to_string(),
            path_prefix: Some(prefix.to_string()),
        }
    }

    #[tokio::test]
    async fn add_and_resolve() {
        let registry = TenantRegistry::new();
        registry.add(sample("shop.example.com")).await.unwrap();

        let resolved = registry.resolve("shop.example.com", "/").await;
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().config.host, "shop.example.com");
    }

    #[tokio::test]
    async fn resolve_strips_port() {
        let registry = TenantRegistry::new();
        registry.add(sample("shop.example.com")).await.unwrap();

        let resolved = registry.resolve("shop.example.com:8080", "/").await;
        assert!(resolved.is_some());
    }

    #[tokio::test]
    async fn resolve_unknown_host_is_none() {
        let registry = TenantRegistry::new();
        assert!(registry.resolve("unknown.example.com", "/").await.is_none());
    }

    #[tokio::test]
    async fn add_duplicate_fails() {
        let registry = TenantRegistry::new();
        registry.add(sample("a.example.com")).await.unwrap();
        let err = registry.add(sample("a.example.com")).await.unwrap_err();
        assert!(matches!(err, TenantError::AlreadyExists(_)));
    }

    #[tokio::test]
    async fn remove_missing_fails() {
        let registry = TenantRegistry::new();
        let err = registry.remove("nope.example.com").await.unwrap_err();
        assert!(matches!(err, TenantError::NotFound(_)));
    }

    #[tokio::test]
    async fn remove_then_resolve_is_none() {
        let registry = TenantRegistry::new();
        registry.add(sample("a.example.com")).await.unwrap();
        registry.remove("a.example.com").await.unwrap();
        assert!(registry.resolve("a.example.com", "/").await.is_none());
    }

    #[tokio::test]
    async fn upsert_replaces_existing() {
        let registry = TenantRegistry::new();
        registry.add(sample("a.example.com")).await.unwrap();

        let mut updated = sample("a.example.com");
        updated.backend_addr = "127.0.0.1:9999".to_string();
        registry.upsert(updated).await;

        let resolved = registry.resolve("a.example.com", "/").await.unwrap();
        assert_eq!(resolved.config.backend_addr, "127.0.0.1:9999");
    }

    /// 同一Hostに複数の`path_prefix`テナントを共存登録し、パスに応じて
    /// 正しく振り分けられることを確認する("分身の術"の対象拡大、
    /// 2026-07-22)。
    #[tokio::test]
    async fn path_prefix_routes_to_the_matching_backend_under_one_host() {
        let registry = TenantRegistry::new();
        registry
            .add(sample_prefixed("runo.tokyo", "/blog", "127.0.0.1:8101"))
            .await
            .unwrap();
        registry
            .add(sample_prefixed("runo.tokyo", "/chiketto", "127.0.0.1:8100"))
            .await
            .unwrap();
        registry
            .add(sample_prefixed("runo.tokyo", "/ec", "127.0.0.1:8102"))
            .await
            .unwrap();

        let blog = registry.resolve("runo.tokyo", "/blog/posts/1").await.unwrap();
        assert_eq!(blog.config.backend_addr, "127.0.0.1:8101");

        let chiketto = registry.resolve("runo.tokyo", "/chiketto").await.unwrap();
        assert_eq!(chiketto.config.backend_addr, "127.0.0.1:8100");

        let ec = registry.resolve("runo.tokyo", "/ec/").await.unwrap();
        assert_eq!(ec.config.backend_addr, "127.0.0.1:8102");

        // 未登録のプレフィックス・ホスト無しのフォールバック無しなら None。
        assert!(registry.resolve("runo.tokyo", "/other").await.is_none());
    }

    /// path_prefix指定のテナントと、Hostのみ(prefix未指定)のテナントが
    /// 同居できる。prefixに一致すればそちらを優先、しなければ
    /// prefix無しの方へフォールバックする(既存の1Host1テナント運用との
    /// 後方互換)。
    #[tokio::test]
    async fn path_prefix_falls_back_to_host_only_tenant() {
        let registry = TenantRegistry::new();
        registry
            .add(sample("runo.tokyo")) // path_prefix無し(従来運用のトップページ相当)
            .await
            .unwrap();
        registry
            .add(sample_prefixed("runo.tokyo", "/blog", "127.0.0.1:8101"))
            .await
            .unwrap();

        let top = registry.resolve("runo.tokyo", "/").await.unwrap();
        assert_eq!(top.config.backend_addr, "127.0.0.1:9001");

        let blog = registry.resolve("runo.tokyo", "/blog").await.unwrap();
        assert_eq!(blog.config.backend_addr, "127.0.0.1:8101");
    }

    /// 同一Host+同一path_prefixの重複登録は拒否されるが、prefixが違えば
    /// 別テナントとして共存できる(逆に言えば重複拒否はprefix込みで判定
    /// されることの確認)。
    #[tokio::test]
    async fn add_duplicate_same_prefix_fails_but_different_prefix_succeeds() {
        let registry = TenantRegistry::new();
        registry
            .add(sample_prefixed("runo.tokyo", "/blog", "127.0.0.1:8101"))
            .await
            .unwrap();
        let err = registry
            .add(sample_prefixed("runo.tokyo", "/blog", "127.0.0.1:9999"))
            .await
            .unwrap_err();
        assert!(matches!(err, TenantError::AlreadyExists(_)));

        registry
            .add(sample_prefixed("runo.tokyo", "/ec", "127.0.0.1:8102"))
            .await
            .unwrap();
        assert_eq!(registry.len().await, 2);
    }

    /// `remove_prefixed`で個別のprefixテナントだけを削除でき、残りの
    /// テナントには影響しない。
    #[tokio::test]
    async fn remove_prefixed_removes_only_that_entry() {
        let registry = TenantRegistry::new();
        registry
            .add(sample_prefixed("runo.tokyo", "/blog", "127.0.0.1:8101"))
            .await
            .unwrap();
        registry
            .add(sample_prefixed("runo.tokyo", "/ec", "127.0.0.1:8102"))
            .await
            .unwrap();

        registry
            .remove_prefixed("runo.tokyo", Some("/blog"))
            .await
            .unwrap();

        assert!(registry.resolve("runo.tokyo", "/blog").await.is_none());
        assert!(registry.resolve("runo.tokyo", "/ec").await.is_some());
    }

    #[test]
    fn strip_path_prefix_variants() {
        assert_eq!(strip_path_prefix("/blog", "/blog"), "/");
        assert_eq!(strip_path_prefix("/blog/", "/blog"), "/");
        assert_eq!(strip_path_prefix("/blog/posts/1", "/blog"), "/posts/1");
        assert_eq!(strip_path_prefix("/other", "/blog"), "/other");
    }

    #[tokio::test]
    async fn load_from_toml_bulk_provisioning() {
        let registry = TenantRegistry::new();
        let toml_str = r#"
            [[domain]]
            host = "shop.example.com"
            backend = "open_runo"
            backend_addr = "127.0.0.1:9001"
            db_uri = "postgres://localhost/shop"

            [[domain]]
            host = "app.example.com"
            backend = "poem_cosmo_tauri"
            backend_addr = "127.0.0.1:9002"
            db_uri = "postgres://localhost/app"
        "#;

        let count = registry.load_from_toml(toml_str).await.unwrap();
        assert_eq!(count, 2);
        assert_eq!(registry.len().await, 2);
        assert!(registry.resolve("shop.example.com", "/").await.is_some());
        assert!(registry.resolve("app.example.com", "/").await.is_some());
    }

    #[tokio::test]
    async fn list_returns_all_configs() {
        let registry = TenantRegistry::new();
        registry.add(sample("a.example.com")).await.unwrap();
        registry.add(sample("b.example.com")).await.unwrap();
        let list = registry.list().await;
        assert_eq!(list.len(), 2);
    }
}
