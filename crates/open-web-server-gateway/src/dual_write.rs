//! 自社ドメインサブドメイン登録の PostgreSQL + aruaru-db デュアルライト。
//!
//! `open-web-server`側「拡張要件(4) DATABASE書き込みの四重化」のうち
//! **①PostgreSQL(ACIDトランザクション、権威)・②aruaru-db(Git-on-SQL、
//! 変更履歴のバージョン管理)の2系統のみ**をこの新機能でも踏襲する
//! (マルチリージョン同期・独立監査ログまでは今回の1パスでは過剰、との
//! ユーザー指示どおり)。
//!
//! エンドポイント自体はVersionLess(バージョン番号をURLに含めない)と
//! しつつ、aruaru-db側のコミット履歴で変更履歴を追える設計にする
//! (open-web-server側の「VersionLessAPI+Git管理ハイブリッド」の考え方を
//! そのまま踏襲)——[`AruaruDbBackend::record_change`]が返す
//! `commit_id`がその役割を担う。
//!
//! ## 正直な開示
//!
//! この開発環境に到達可能な実PostgreSQL/aruaru-dbインスタンスは
//! 確認できなかった(`open-web-server-ledger::postgres_wal`が既に
//! 記録している既知の制約と同じ環境)。そのため今回は単体テスト+モックに
//! よる検証に留め、実DB接続でのE2Eは実施していない。

use async_trait::async_trait;

/// 登録済みサブドメイン1件分のレコード。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SubdomainRecord {
    pub owner_account_id: String,
    pub subdomain_name: String,
    pub base_domain: String,
    pub current_ip: String,
    /// RFC3339形式。`chrono`は既にワークスペース依存にあるため追加依存は無し。
    pub created_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DualWriteError {
    #[error("PostgreSQL write failed: {0}")]
    PostgresFailed(String),
    #[error("aruaru-db write failed: {0}")]
    AruaruDbFailed(String),
}

/// PostgreSQL側の書き込み口を抽象化するトレイト(実PostgreSQL実装・
/// モック実装どちらも差し込めるようにする、`open-web-server-ledger::
/// WriteAheadLog`と同じ設計思想)。
#[async_trait]
pub trait PostgresBackend: Send + Sync {
    async fn insert_subdomain(&self, record: &SubdomainRecord) -> Result<(), DualWriteError>;
}

/// aruaru-db側の書き込み口を抽象化するトレイト。`record_change`は
/// Git-on-SQLのコミットIDを返す(VersionLessAPI+Git管理ハイブリッドの
/// 「変更履歴」を担う部分)。
#[async_trait]
pub trait AruaruDbBackend: Send + Sync {
    async fn record_change(&self, record: &SubdomainRecord) -> Result<String, DualWriteError>;
}

/// PostgreSQL実装(`sqlx`、`custom_domain_db` feature配下でのみコンパイル)。
/// `open-web-server-ledger::PostgresWal`と同じ「実`BEGIN`/`COMMIT`
/// トランザクション境界+`ON CONFLICT DO NOTHING`による冪等書き込み」の
/// パターンを踏襲する。
#[cfg(feature = "custom_domain_db")]
pub struct SqlxPostgresBackend {
    pool: sqlx::PgPool,
}

#[cfg(feature = "custom_domain_db")]
impl SqlxPostgresBackend {
    pub const SCHEMA_SQL: &'static str = r#"
CREATE TABLE IF NOT EXISTS custom_subdomains (
    owner_account_id TEXT NOT NULL,
    subdomain_name   TEXT NOT NULL,
    base_domain      TEXT NOT NULL,
    current_ip       TEXT NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (subdomain_name, base_domain)
)
"#;

    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = sqlx::PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    pub async fn ensure_schema(&self) -> anyhow::Result<()> {
        sqlx::query(Self::SCHEMA_SQL).execute(&self.pool).await?;
        Ok(())
    }
}

#[cfg(feature = "custom_domain_db")]
#[async_trait]
impl PostgresBackend for SqlxPostgresBackend {
    async fn insert_subdomain(&self, record: &SubdomainRecord) -> Result<(), DualWriteError> {
        let mut tx = self.pool.begin().await.map_err(|e| DualWriteError::PostgresFailed(e.to_string()))?;
        sqlx::query(
            "INSERT INTO custom_subdomains (owner_account_id, subdomain_name, base_domain, current_ip, created_at) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (subdomain_name, base_domain) DO UPDATE SET current_ip = EXCLUDED.current_ip",
        )
        .bind(&record.owner_account_id)
        .bind(&record.subdomain_name)
        .bind(&record.base_domain)
        .bind(&record.current_ip)
        .bind(
            chrono::DateTime::parse_from_rfc3339(&record.created_at)
                .map_err(|e| DualWriteError::PostgresFailed(e.to_string()))?,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| DualWriteError::PostgresFailed(e.to_string()))?;
        tx.commit().await.map_err(|e| DualWriteError::PostgresFailed(e.to_string()))?;
        Ok(())
    }
}

/// aruaru-db実装。実接続先は`open-runo`のFederation Gateway経由の想定だが、
/// このリポジトリ単体からは`open-web-server-ledger::DbStateReader`と同じ
/// 制約(到達可能な実aruaru-dbインスタンスがこの開発環境に無い)のため、
/// 実HTTP呼び出しコード自体は`custom_domain` feature配下に置きつつ、
/// 実接続検証は未実施であることを明記する。
pub struct HttpAruaruDbBackend {
    endpoint: String,
    #[cfg(feature = "custom_domain")]
    client: reqwest::Client,
    /// open-runoの`selfIssueKey`で自動発行した短命キー(期限切れは1回だけ再発行して再試行)。
    #[cfg(feature = "custom_domain")]
    api_key: tokio::sync::Mutex<Option<String>>,
}

impl HttpAruaruDbBackend {
    pub const ENV_ENDPOINT: &str = "OPEN_EASY_WEB_ARUARU_DB_ENDPOINT";

    pub fn from_env() -> Result<Self, DualWriteError> {
        let endpoint = std::env::var(Self::ENV_ENDPOINT)
            .map_err(|_| DualWriteError::AruaruDbFailed(format!("{} is not set", Self::ENV_ENDPOINT)))?;
        Ok(Self {
            endpoint,
            #[cfg(feature = "custom_domain")]
            client: reqwest::Client::new(),
            #[cfg(feature = "custom_domain")]
            api_key: tokio::sync::Mutex::new(None),
        })
    }

    /// `POST {endpoint}/open_runo.admin.v1.Admin/<method>`(Connect、JSON)を1回叩く。
    /// 2xxなら`Ok(Ok(JSON))`、それ以外は`Ok(Err((HTTPステータス, メッセージ)))`。
    #[cfg(feature = "custom_domain")]
    async fn rpc(
        &self,
        method: &str,
        body: serde_json::Value,
        api_key: Option<&str>,
    ) -> Result<Result<serde_json::Value, (u16, String)>, DualWriteError> {
        let url = format!(
            "{}/open_runo.admin.v1.Admin/{method}",
            self.endpoint.trim_end_matches('/')
        );
        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .header("connect-protocol-version", "1")
            .json(&body);
        if let Some(key) = api_key {
            req = req.header("x-api-key", key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| DualWriteError::AruaruDbFailed(e.to_string()))?;
        let status = resp.status().as_u16();
        let json: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        if (200..300).contains(&status) {
            return Ok(Ok(json));
        }
        Ok(Err((status, json["message"].as_str().unwrap_or("").to_string())))
    }

    #[cfg(feature = "custom_domain")]
    async fn issue_key(&self) -> Result<String, DualWriteError> {
        let data = self
            .rpc("SelfIssueKey", serde_json::json!({}), None)
            .await?
            .map_err(|(s, m)| DualWriteError::AruaruDbFailed(format!("self-issue failed ({s}): {m}")))?;
        let key = data["apiKey"]
            .as_str()
            .ok_or_else(|| DualWriteError::AruaruDbFailed("self-issue response missing api_key".to_string()))?
            .to_string();
        *self.api_key.lock().await = Some(key.clone());
        Ok(key)
    }
}

#[async_trait]
impl AruaruDbBackend for HttpAruaruDbBackend {
    #[cfg(feature = "custom_domain")]
    async fn record_change(&self, record: &SubdomainRecord) -> Result<String, DualWriteError> {
        let body = serde_json::json!({
            "table": "custom_subdomains",
            "key": record.subdomain_name,
            "value": serde_json::to_value(record).map_err(|e| DualWriteError::AruaruDbFailed(e.to_string()))?,
        });

        let cached = self.api_key.lock().await.clone();
        let mut key = match cached {
            Some(k) => k,
            None => self.issue_key().await?,
        };
        let mut outcome = self.rpc("DbPut", body.clone(), Some(&key)).await?;
        if matches!(outcome, Err((401, _))) {
            // キャッシュ済みキーの期限切れ: 1回だけ再発行して再試行する。
            key = self.issue_key().await?;
            outcome = self.rpc("DbPut", body, Some(&key)).await?;
        }
        let data = outcome.map_err(|(s, m)| DualWriteError::AruaruDbFailed(format!("HTTP {s}: {m}")))?;
        // open-runoのDbPutは現状コミットIDを返さない(`commit_id`が付いていれば使う)。
        data["commit_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| DualWriteError::AruaruDbFailed("open-runo response missing commit_id".to_string()))
    }

    #[cfg(not(feature = "custom_domain"))]
    async fn record_change(&self, _record: &SubdomainRecord) -> Result<String, DualWriteError> {
        Err(DualWriteError::AruaruDbFailed(
            "this build was compiled without the `custom_domain` feature (no HTTP client available)".to_string(),
        ))
    }
}

/// PostgreSQL・aruaru-dbの両方へ書き込む調整役。**PostgreSQLを権威パスと
/// 位置づけ、そちらが失敗した場合は全体を失敗として扱う**
/// (`open-web-server-ledger::multi_region::MultiRegionReplicator`の
/// 「厳格モード」に近い設計判断)。aruaru-db側の書き込みが失敗しても
/// PostgreSQL側は既にコミット済みのため、呼び出し側には
/// `partial_failure`として正直に報告する(黙って握りつぶさない)。
pub struct DualWriteCoordinator<P: PostgresBackend, A: AruaruDbBackend> {
    postgres: P,
    aruaru_db: A,
}

#[derive(Debug, Clone)]
pub struct DualWriteOutcome {
    pub postgres_committed: bool,
    pub aruaru_db_commit_id: Option<String>,
    pub aruaru_db_error: Option<String>,
}

impl<P: PostgresBackend, A: AruaruDbBackend> DualWriteCoordinator<P, A> {
    pub fn new(postgres: P, aruaru_db: A) -> Self {
        Self { postgres, aruaru_db }
    }

    pub async fn write(&self, record: &SubdomainRecord) -> Result<DualWriteOutcome, DualWriteError> {
        self.postgres.insert_subdomain(record).await?;
        match self.aruaru_db.record_change(record).await {
            Ok(commit_id) => Ok(DualWriteOutcome { postgres_committed: true, aruaru_db_commit_id: Some(commit_id), aruaru_db_error: None }),
            Err(e) => Ok(DualWriteOutcome { postgres_committed: true, aruaru_db_commit_id: None, aruaru_db_error: Some(e.to_string()) }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockPostgres {
        inserted: Mutex<Vec<SubdomainRecord>>,
        fail: bool,
    }

    #[async_trait]
    impl PostgresBackend for MockPostgres {
        async fn insert_subdomain(&self, record: &SubdomainRecord) -> Result<(), DualWriteError> {
            if self.fail {
                return Err(DualWriteError::PostgresFailed("simulated failure".to_string()));
            }
            self.inserted.lock().unwrap().push(record.clone());
            Ok(())
        }
    }

    struct MockAruaruDb {
        next_commit_id: String,
        fail: bool,
    }

    #[async_trait]
    impl AruaruDbBackend for MockAruaruDb {
        async fn record_change(&self, _record: &SubdomainRecord) -> Result<String, DualWriteError> {
            if self.fail {
                return Err(DualWriteError::AruaruDbFailed("simulated failure".to_string()));
            }
            Ok(self.next_commit_id.clone())
        }
    }

    fn sample_record() -> SubdomainRecord {
        SubdomainRecord {
            owner_account_id: "github:42".to_string(),
            subdomain_name: "myapp".to_string(),
            base_domain: "aon.co.jp".to_string(),
            current_ip: "203.0.113.5".to_string(),
            created_at: "2026-07-24T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn dual_write_succeeds_on_both_backends() {
        let coordinator = DualWriteCoordinator::new(
            MockPostgres { inserted: Mutex::new(Vec::new()), fail: false },
            MockAruaruDb { next_commit_id: "commit-abc123".to_string(), fail: false },
        );
        let outcome = coordinator.write(&sample_record()).await.unwrap();
        assert!(outcome.postgres_committed);
        assert_eq!(outcome.aruaru_db_commit_id.as_deref(), Some("commit-abc123"));
        assert!(outcome.aruaru_db_error.is_none());
        assert_eq!(coordinator.postgres.inserted.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn postgres_failure_aborts_the_whole_write() {
        let coordinator = DualWriteCoordinator::new(
            MockPostgres { inserted: Mutex::new(Vec::new()), fail: true },
            MockAruaruDb { next_commit_id: "unused".to_string(), fail: false },
        );
        let err = coordinator.write(&sample_record()).await.expect_err("postgres failure must propagate");
        assert!(matches!(err, DualWriteError::PostgresFailed(_)));
    }

    #[tokio::test]
    async fn aruaru_db_failure_is_reported_honestly_without_hiding_the_committed_postgres_write() {
        let coordinator = DualWriteCoordinator::new(
            MockPostgres { inserted: Mutex::new(Vec::new()), fail: false },
            MockAruaruDb { next_commit_id: "unused".to_string(), fail: true },
        );
        let outcome = coordinator.write(&sample_record()).await.unwrap();
        assert!(outcome.postgres_committed, "postgres write must still be reported as committed");
        assert!(outcome.aruaru_db_commit_id.is_none());
        assert!(outcome.aruaru_db_error.is_some(), "aruaru-db failure must be surfaced, not swallowed");
    }

    /// `HttpAruaruDbBackend`がopen-runoのGraphQL(`selfIssueKey`→`dbPut`)を実TCP+実HTTPで
    /// 叩くこと。期限切れキー(モックが`extensions.status`=401を返す)は1回だけ再発行して再試行する。
    #[cfg(feature = "custom_domain")]
    #[tokio::test]
    async fn http_backend_writes_via_connect_json_and_reissues_an_expired_key_once() {
        use bytes::Bytes;
        use http_body_util::{BodyExt, Full};
        use hyper::body::Incoming;
        use hyper::server::conn::http1;
        use hyper::service::service_fn;
        use hyper::{Request, Response, StatusCode};
        use hyper_util::rt::TokioIo;
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let issues = Arc::new(AtomicU32::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let counter = Arc::clone(&issues);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { continue };
                let counter = Arc::clone(&counter);
                tokio::spawn(async move {
                    let service = service_fn(move |req: Request<Incoming>| {
                        let counter = Arc::clone(&counter);
                        async move {
                            let is_self_issue = req.uri().path().ends_with("/SelfIssueKey");
                            let key = req
                                .headers()
                                .get("x-api-key")
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("")
                                .to_string();
                            let bytes = req.into_body().collect().await.map(|c| c.to_bytes()).unwrap_or_default();
                            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
                            let (status, payload) = if is_self_issue {
                                counter.fetch_add(1, Ordering::SeqCst);
                                (200u16, serde_json::json!({ "apiKey": "fresh-key" }))
                            } else if key != "fresh-key" {
                                (401, serde_json::json!({ "code": "unauthenticated", "message": "authentication required" }))
                            } else if body["table"] == "custom_subdomains" && body["key"] == "alice" {
                                (200, serde_json::json!({ "table": "custom_subdomains", "key": "alice", "value": {}, "commit_id": "c-42" }))
                            } else {
                                (422, serde_json::json!({ "code": "invalid_argument", "message": "unexpected" }))
                            };
                            Ok::<_, std::convert::Infallible>(
                                Response::builder()
                                    .status(StatusCode::from_u16(status).unwrap())
                                    .header("content-type", "application/json")
                                    .body(Full::new(Bytes::from(payload.to_string())))
                                    .unwrap(),
                            )
                        }
                    });
                    let _ = http1::Builder::new().serve_connection(TokioIo::new(stream), service).await;
                });
            }
        });

        let backend = HttpAruaruDbBackend {
            endpoint: format!("http://{addr}"),
            client: reqwest::Client::new(),
            api_key: tokio::sync::Mutex::new(Some("stale-key".to_string())),
        };
        let mut record = sample_record();
        record.subdomain_name = "alice".to_string();
        let commit_id = backend.record_change(&record).await.expect("401 -> reissue -> retry should succeed");
        assert_eq!(commit_id, "c-42");
        assert_eq!(issues.load(Ordering::SeqCst), 1, "exactly one re-issue");
        // 2回目は再発行不要(キャッシュ済み)。
        backend.record_change(&record).await.unwrap();
        assert_eq!(issues.load(Ordering::SeqCst), 1);
    }
}
