//! VersionLessAPI + Git-on-SQL ハイブリッドの読み出し側 (拡張要件(1))。
//!
//! `Ledger::commit`(書き込み側、`forward_once`経由でopen-runoへ確定を
//! フォワードする)の対になる、履歴読み出し側。ある target/key が指定
//! コミット時点で保持していた値を、open-runoのGraphQL
//! (`POST /graphql` の `query { admin { dbGetAtCommit(...) } }`、
//! `AruaruDbBackend`のみ実対応で他バックエンドは`extensions.status`=501)
//! へ問い合わせて返す。旧RESTの`GET /api/db/:table/:key/at/:commit_id`は
//! open-runo側のREST全廃(2026-11-01)で廃止予定のため、こちらへ切り替えた。
//! `open-web-server-gateway`側の
//! `GET /internal/db/state/:target/:key/at/:commit_id`ハンドラから使われる。
//!
//! **認証**: open-runoの`admin`は`X-Api-Key`(またはセッションCookie)を要求する。
//! `open-runo-cli`・WASMフロントエンドと同じ「人間がAPIキーを意識しない」方針:
//! 初回利用時に`mutation { selfIssueKey }`で短命な developer role キーを自動発行
//! しキャッシュ、認証失敗(GraphQLエラーの`extensions.status`=401。HTTPは200の
//! ままなので注意)が返ってきた場合は透過的に再発行してリトライする。

use std::sync::Arc;

use open_web_server_core::DbStateAtCommitResponse;
use tokio::sync::Mutex;

/// open-runoへの読み出しプロキシ。`Ledger`とは独立した構造体
/// (書き込みパス(`forward_once`)と読み出しパスは異なる関心事であり、
/// 将来的に別々のopen-runoインスタンス/リージョンを指す可能性もある
/// ため、意図的に結合していない)。
pub struct DbStateReader {
    http: reqwest::Client,
    open_runo_endpoint: String,
    cached_api_key: Mutex<Option<String>>,
}

impl DbStateReader {
    pub fn new(open_runo_endpoint: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            open_runo_endpoint,
            cached_api_key: Mutex::new(None),
        }
    }

    pub fn shared(open_runo_endpoint: String) -> Arc<Self> {
        Arc::new(Self::new(open_runo_endpoint))
    }

    /// `target/key`が`commit_id`時点で保持していた値を取得する。
    /// `Ok(None)`はopen-runoが404を返したことを意味する(コミット不明、
    /// またはその時点でキーがまだ存在しなかった)——これは正常な結果で
    /// あり、エラーではない。
    pub async fn get_at_commit(
        &self,
        target: &str,
        key: &str,
        commit_id: &str,
    ) -> anyhow::Result<Option<DbStateAtCommitResponse>> {
        let api_key = self.ensure_api_key().await?;
        let mut outcome = self.request(target, key, commit_id, &api_key).await?;

        if matches!(outcome, Gql::Status(401, _)) {
            // キャッシュ済みキーが期限切れ(self-issueキーは短命)——
            // 一度だけ再発行してリトライする。呼び出し元には
            // 「たまたま期限が切れていた」という実装詳細を露出しない。
            let fresh_key = self.self_issue_key().await?;
            *self.cached_api_key.lock().await = Some(fresh_key.clone());
            outcome = self.request(target, key, commit_id, &fresh_key).await?;
        }

        Self::parse_response(target, key, commit_id, outcome)
    }

    /// `POST /graphql`を1回叩く。HTTPが2xx以外ならエラー、GraphQLエラーは
    /// `extensions.status`(open-runoがRESTのHTTPステータスと同じ値を載せる)付きで返す。
    async fn gql(
        &self,
        query: &str,
        variables: serde_json::Value,
        api_key: Option<&str>,
    ) -> anyhow::Result<Gql> {
        let url = format!("{}/graphql", self.open_runo_endpoint.trim_end_matches('/'));
        let mut req = self
            .http
            .post(url)
            .json(&serde_json::json!({ "query": query, "variables": variables }));
        if let Some(key) = api_key {
            req = req.header("x-api-key", key);
        }
        let body: serde_json::Value = req.send().await?.error_for_status()?.json().await?;
        if let Some(err) = body["errors"].get(0) {
            let status = err["extensions"]["status"].as_u64().unwrap_or(0) as u16;
            let message = err["message"].as_str().unwrap_or("GraphQL error").to_string();
            return Ok(Gql::Status(status, message));
        }
        Ok(Gql::Data(body["data"].clone()))
    }

    async fn request(
        &self,
        target: &str,
        key: &str,
        commit_id: &str,
        api_key: &str,
    ) -> anyhow::Result<Gql> {
        self.gql(
            "query ($t: String!, $k: String!, $c: String!) { admin { dbGetAtCommit(table: $t, key: $k, commitId: $c) } }",
            serde_json::json!({ "t": target, "k": key, "c": commit_id }),
            Some(api_key),
        )
        .await
    }

    fn parse_response(
        target: &str,
        key: &str,
        commit_id: &str,
        outcome: Gql,
    ) -> anyhow::Result<Option<DbStateAtCommitResponse>> {
        match outcome {
            Gql::Data(data) => {
                let record = &data["admin"]["dbGetAtCommit"];
                let value = record.get("value").cloned().unwrap_or(serde_json::Value::Null);
                Ok(Some(DbStateAtCommitResponse {
                    target: target.to_string(),
                    key: key.to_string(),
                    commit_id: commit_id.to_string(),
                    value,
                }))
            }
            Gql::Status(404, _) => Ok(None),
            // 501 (バックエンドがコミット履歴クエリ自体に未対応) を含め、
            // それ以外の全ステータスは呼び出し元にエラーとして伝える。
            Gql::Status(other, message) => Err(anyhow::anyhow!(
                "open-runo returned unexpected status {other} for {target}/{key}@{commit_id}: {message}"
            )),
        }
    }

    async fn ensure_api_key(&self) -> anyhow::Result<String> {
        if let Some(key) = self.cached_api_key.lock().await.clone() {
            return Ok(key);
        }
        let key = self.self_issue_key().await?;
        *self.cached_api_key.lock().await = Some(key.clone());
        Ok(key)
    }

    async fn self_issue_key(&self) -> anyhow::Result<String> {
        match self
            .gql("mutation { selfIssueKey }", serde_json::json!({}), None)
            .await?
        {
            Gql::Data(data) => data["selfIssueKey"]["api_key"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow::anyhow!("open-runo self-issue response missing api_key")),
            Gql::Status(status, message) => {
                Err(anyhow::anyhow!("open-runo self-issue failed ({status}): {message}"))
            }
        }
    }
}

/// `DbStateReader::gql`の結果。
enum Gql {
    Data(serde_json::Value),
    Status(u16, String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http_body_util::Full;
    use hyper::body::Incoming;
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Method, Request, Response, StatusCode};
    use hyper_util::rt::TokioIo;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tokio::net::TcpListener;

    /// 最小限のopen-runoモック(`POST /graphql`のみ):
    /// `selfIssueKey`(呼び出し回数を記録)と`dbGetAtCommit`(固定のtarget/key/
    /// commit_idにのみ値を返し、それ以外は`extensions.status`=404のGraphQLエラー。
    /// 実際のopen-runoと同じくHTTPは常に200)。`X-Api-Key`が
    /// `self-issued-test-key`でなければ`extensions.status`=401を返す。
    /// 実TCP+実HTTPで`DbStateReader`を検証するための最小サーバー。
    async fn spawn_mock_open_runo() -> (std::net::SocketAddr, Arc<AtomicU32>) {
        let self_issue_calls = Arc::new(AtomicU32::new(0));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let calls = Arc::clone(&self_issue_calls);
        tokio::spawn(async move {
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => continue,
                };
                let io = TokioIo::new(stream);
                let calls = Arc::clone(&calls);
                tokio::spawn(async move {
                    let service = service_fn(move |req: Request<Incoming>| {
                        let calls = Arc::clone(&calls);
                        async move {
                            let method = req.method().clone();
                            let path = req.uri().path().to_string();
                            let key = req
                                .headers()
                                .get("x-api-key")
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("")
                                .to_string();
                            let bytes = http_body_util::BodyExt::collect(req.into_body())
                                .await
                                .map(|c| c.to_bytes())
                                .unwrap_or_default();
                            let body: serde_json::Value =
                                serde_json::from_slice(&bytes).unwrap_or_default();
                            let query = body["query"].as_str().unwrap_or("").to_string();
                            let vars = body["variables"].clone();
                            let gql_error = |status: u16| {
                                serde_json::json!({
                                    "data": null,
                                    "errors": [{ "message": "mock", "extensions": { "status": status } }],
                                })
                            };
                            let payload = if method != Method::POST || path != "/graphql" {
                                None
                            } else if query.contains("selfIssueKey") {
                                calls.fetch_add(1, Ordering::SeqCst);
                                Some(serde_json::json!({
                                    "data": { "selfIssueKey": {
                                        "api_key": "self-issued-test-key",
                                        "expires_at": "2099-01-01T00:00:00Z",
                                    } },
                                }))
                            } else if query.contains("dbGetAtCommit") {
                                if key != "self-issued-test-key" {
                                    Some(gql_error(401))
                                } else if vars["t"] == "game_items"
                                    && vars["k"] == "player-1"
                                    && vars["c"] == "commit-abc"
                                {
                                    Some(serde_json::json!({
                                        "data": { "admin": { "dbGetAtCommit": {
                                            "table": "game_items",
                                            "key": "player-1",
                                            "commit_id": "commit-abc",
                                            "value": { "quantity": 3 },
                                        } } },
                                    }))
                                } else {
                                    Some(gql_error(404))
                                }
                            } else {
                                None
                            };
                            let resp = match payload {
                                Some(p) => Response::builder()
                                    .status(StatusCode::OK)
                                    .header("content-type", "application/json")
                                    .body(Full::new(Bytes::from(p.to_string())))
                                    .unwrap(),
                                None => Response::builder()
                                    .status(StatusCode::NOT_FOUND)
                                    .body(Full::new(Bytes::new()))
                                    .unwrap(),
                            };
                            Ok::<_, std::convert::Infallible>(resp)
                        }
                    });
                    let _ = http1::Builder::new().serve_connection(io, service).await;
                });
            }
        });

        (addr, self_issue_calls)
    }

    #[tokio::test]
    async fn get_at_commit_returns_the_real_historical_value_over_real_http() {
        let (addr, _self_issue_calls) = spawn_mock_open_runo().await;
        let reader = DbStateReader::new(format!("http://{addr}"));

        let result = reader
            .get_at_commit("game_items", "player-1", "commit-abc")
            .await
            .expect("request should succeed");
        let response = result.expect("known target/key/commit should return Some");
        assert_eq!(response.target, "game_items");
        assert_eq!(response.key, "player-1");
        assert_eq!(response.commit_id, "commit-abc");
        assert_eq!(response.value, serde_json::json!({ "quantity": 3 }));
    }

    #[tokio::test]
    async fn get_at_commit_returns_none_for_unknown_commit() {
        let (addr, _self_issue_calls) = spawn_mock_open_runo().await;
        let reader = DbStateReader::new(format!("http://{addr}"));

        let result = reader
            .get_at_commit("game_items", "player-1", "no-such-commit")
            .await
            .expect("request should succeed even when open-runo 404s");
        assert!(result.is_none(), "unknown commit should be Ok(None), not an error");
    }

    #[tokio::test]
    async fn get_at_commit_self_issues_a_key_exactly_once_across_multiple_requests() {
        let (addr, self_issue_calls) = spawn_mock_open_runo().await;
        let reader = DbStateReader::new(format!("http://{addr}"));

        reader.get_at_commit("game_items", "player-1", "commit-abc").await.unwrap();
        reader.get_at_commit("game_items", "player-1", "commit-abc").await.unwrap();
        reader.get_at_commit("game_items", "player-1", "commit-abc").await.unwrap();

        assert_eq!(
            self_issue_calls.load(Ordering::SeqCst),
            1,
            "the self-issued key should be cached and reused, not re-issued on every request"
        );
    }

    #[tokio::test]
    async fn expired_cached_key_is_reissued_transparently_on_401() {
        let (addr, self_issue_calls) = spawn_mock_open_runo().await;
        let reader = DbStateReader::new(format!("http://{addr}"));
        // 期限切れ(モックが401を返す)キーをキャッシュに仕込む。
        *reader.cached_api_key.lock().await = Some("stale-key".to_string());

        let result = reader
            .get_at_commit("game_items", "player-1", "commit-abc")
            .await
            .expect("a 401 should trigger exactly one re-issue and retry");
        assert!(result.is_some());
        assert_eq!(self_issue_calls.load(Ordering::SeqCst), 1);
    }
}
