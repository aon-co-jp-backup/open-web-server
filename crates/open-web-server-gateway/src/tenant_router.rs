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
    tenants: RwLock<HashMap<String, Arc<TenantHandle>>>,
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
    async fn persist(&self, tenants: &HashMap<String, Arc<TenantHandle>>) {
        let Some(path) = self.persist_path.read().await.clone() else {
            return;
        };

        let domains: Vec<TenantConfig> = tenants.values().map(|h| h.config.clone()).collect();
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
            guard.insert(
                config.host.clone(),
                Arc::new(TenantHandle { config }),
            );
        }
        Ok(count)
    }

    /// ドメインを1件追加する(ノーダウンタイム、既存接続には影響しない)。
    /// 指定ホスト(サブドメイン含むフルホスト名)が既に登録済みか。
    /// `update_tenant`ハンドラで「変更」と「新規追加」を区別するために使う。
    pub async fn exists(&self, host: &str) -> bool {
        self.tenants.read().await.contains_key(host)
    }

    pub async fn add(&self, config: TenantConfig) -> Result<(), TenantError> {
        let mut guard = self.tenants.write().await;
        if guard.contains_key(&config.host) {
            return Err(TenantError::AlreadyExists(config.host));
        }
        guard.insert(config.host.clone(), Arc::new(TenantHandle { config }));
        self.persist(&guard).await;
        Ok(())
    }

    /// 既存ドメインの設定を置き換える(存在しなければ追加と同じ扱い)。
    pub async fn upsert(&self, config: TenantConfig) {
        let mut guard = self.tenants.write().await;
        guard.insert(config.host.clone(), Arc::new(TenantHandle { config }));
        self.persist(&guard).await;
    }

    /// ドメインを削除する。
    pub async fn remove(&self, host: &str) -> Result<(), TenantError> {
        let mut guard = self.tenants.write().await;
        let removed = guard.remove(host);
        if removed.is_none() {
            return Err(TenantError::NotFound(host.to_string()));
        }
        self.persist(&guard).await;
        Ok(())
    }

    /// Hostヘッダ(ポート番号が付いている場合は除去して)からテナントを引く。
    pub async fn resolve(&self, host_header: &str) -> Option<Arc<TenantHandle>> {
        let host = host_header.split(':').next().unwrap_or(host_header);
        self.tenants.read().await.get(host).cloned()
    }

    pub async fn list(&self) -> Vec<TenantConfig> {
        self.tenants
            .read()
            .await
            .values()
            .map(|h| h.config.clone())
            .collect()
    }

    pub async fn len(&self) -> usize {
        self.tenants.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.tenants.read().await.is_empty()
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
        }
    }

    #[tokio::test]
    async fn add_and_resolve() {
        let registry = TenantRegistry::new();
        registry.add(sample("shop.example.com")).await.unwrap();

        let resolved = registry.resolve("shop.example.com").await;
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().config.host, "shop.example.com");
    }

    #[tokio::test]
    async fn resolve_strips_port() {
        let registry = TenantRegistry::new();
        registry.add(sample("shop.example.com")).await.unwrap();

        let resolved = registry.resolve("shop.example.com:8080").await;
        assert!(resolved.is_some());
    }

    #[tokio::test]
    async fn resolve_unknown_host_is_none() {
        let registry = TenantRegistry::new();
        assert!(registry.resolve("unknown.example.com").await.is_none());
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
        assert!(registry.resolve("a.example.com").await.is_none());
    }

    #[tokio::test]
    async fn upsert_replaces_existing() {
        let registry = TenantRegistry::new();
        registry.add(sample("a.example.com")).await.unwrap();

        let mut updated = sample("a.example.com");
        updated.backend_addr = "127.0.0.1:9999".to_string();
        registry.upsert(updated).await;

        let resolved = registry.resolve("a.example.com").await.unwrap();
        assert_eq!(resolved.config.backend_addr, "127.0.0.1:9999");
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
        assert!(registry.resolve("shop.example.com").await.is_some());
        assert!(registry.resolve("app.example.com").await.is_some());
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
