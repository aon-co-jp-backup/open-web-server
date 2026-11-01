//! VersionLessAPI + Git-on-SQL ハイブリッドの読み出し側 (拡張要件(1))。
//!
//! `Ledger::commit`(書き込み側、`forward_once`経由でopen-runoへ確定を
//! フォワードする)の対になる、履歴読み出し側。ある target/key が指定
//! コミット時点で保持していた値を、open-runoの管理サービス
//! (`open_runo.admin.v1.Admin`)の`DbGetAtCommit`で問い合わせて返す
//! (`AruaruDbBackend`のみ実対応で他バックエンドは501=`UNIMPLEMENTED`)。
//! 旧RESTの`GET /api/db/:table/:key/at/:commit_id`はopen-runo側のREST全廃(2026-11-01)で
//! 廃止された。`open-web-server`の前面プロキシ越えを考え、**素のHTTP POST(Connectプロトコルの
//! JSON、`POST /open_runo.admin.v1.Admin/<Method>`)**で呼ぶ: HTTP/2もトレーラーも不要で、
//! 既存の`reqwest`だけで動き、エラーはHTTPステータス(404/401/501…)でそのまま判別できる。
//! `open-web-server-gateway`側の`GET /internal/db/state/:target/:key/at/:commit_id`から使われる。
//!
//! **認証**: open-runoの管理サービスは`X-Api-Key`(またはセッションCookie)を要求する。
//! `open-runo-cli`・管理画面と同じ「人間がAPIキーを意識しない」方針: 初回利用時に
//! `SelfIssueKey`で短命な developer role キーを自動発行しキャッシュ、`401`が返ってきた場合
//! (キャッシュ済みキーの期限切れ)は透過的に再発行してリトライする。

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

        if matches!(outcome, Rpc::Status(401, _)) {
            // キャッシュ済みキーが期限切れ(self-issueキーは短命)——
            // 一度だけ再発行してリトライする。呼び出し元には
            // 「たまたま期限が切れていた」という実装詳細を露出しない。
            let fresh_key = self.self_issue_key().await?;
            *self.cached_api_key.lock().await = Some(fresh_key.clone());
            outcome = self.request(target, key, commit_id, &fresh_key).await?;
        }

        Self::parse_response(target, key, commit_id, outcome)
    }

    /// `POST {endpoint}/open_runo.admin.v1.Admin/<method>`(Connect、JSON)を1回叩く。
    /// 2xxなら本文のJSON、それ以外はHTTPステータスとエラーメッセージ(`{code,message}`)で返す。
    async fn rpc(
        &self,
        method: &str,
        body: serde_json::Value,
        api_key: Option<&str>,
    ) -> anyhow::Result<Rpc> {
        let url = format!(
            "{}/open_runo.admin.v1.Admin/{method}",
            self.open_runo_endpoint.trim_end_matches('/')
        );
        let mut req = self
            .http
            .post(url)
            .header("content-type", "application/json")
            .header("connect-protocol-version", "1")
            .json(&body);
        if let Some(key) = api_key {
            req = req.header("x-api-key", key);
        }
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let json: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        if (200..300).contains(&status) {
            return Ok(Rpc::Data(json));
        }
        let message = json["message"].as_str().unwrap_or("").to_string();
        Ok(Rpc::Status(status, message))
    }

    async fn request(
        &self,
        target: &str,
        key: &str,
        commit_id: &str,
        api_key: &str,
    ) -> anyhow::Result<Rpc> {
        self.rpc(
            "DbGetAtCommit",
            serde_json::json!({ "table": target, "key": key, "commitId": commit_id }),
            Some(api_key),
        )
        .await
    }

    fn parse_response(
        target: &str,
        key: &str,
        commit_id: &str,
        outcome: Rpc,
    ) -> anyhow::Result<Option<DbStateAtCommitResponse>> {
        match outcome {
            Rpc::Data(record) => {
                let value = record.get("value").cloned().unwrap_or(serde_json::Value::Null);
                Ok(Some(DbStateAtCommitResponse {
                    target: target.to_string(),
                    key: key.to_string(),
                    commit_id: commit_id.to_string(),
                    value,
                }))
            }
            Rpc::Status(404, _) => Ok(None),
            // 501 (バックエンドがコミット履歴クエリ自体に未対応) を含め、
            // それ以外の全ステータスは呼び出し元にエラーとして伝える。
            Rpc::Status(other, message) => Err(anyhow::anyhow!(
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
        match self.rpc("SelfIssueKey", serde_json::json!({}), None).await? {
            Rpc::Data(data) => data["apiKey"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow::anyhow!("open-runo self-issue response missing apiKey")),
            Rpc::Status(status, message) => {
                Err(anyhow::anyhow!("open-runo self-issue failed ({status}): {message}"))
            }
        }
    }
}

/// `DbStateReader::rpc`の結果。
enum Rpc {
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

    /// 最小限のopen-runoモック(Connect JSON、`POST /open_runo.admin.v1.Admin/<Method>`):
    /// `SelfIssueKey`(呼び出し回数を記録)と`DbGetAtCommit`(固定のtarget/key/commit_idにのみ値を返し、
    /// それ以外は404)。`X-Api-Key`が`self-issued-test-key`でなければ401を返す。
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
                            let json = |status: StatusCode, v: serde_json::Value| {
                                Response::builder()
                                    .status(status)
                                    .header("content-type", "application/json")
                                    .body(Full::new(Bytes::from(v.to_string())))
                                    .unwrap()
                            };
                            let resp = if method != Method::POST {
                                Response::builder().status(StatusCode::NOT_FOUND).body(Full::new(Bytes::new())).unwrap()
                            } else if path == "/open_runo.admin.v1.Admin/SelfIssueKey" {
                                calls.fetch_add(1, Ordering::SeqCst);
                                json(
                                    StatusCode::OK,
                                    serde_json::json!({ "apiKey": "self-issued-test-key", "expiresAt": "2099-01-01T00:00:00Z" }),
                                )
                            } else if path == "/open_runo.admin.v1.Admin/DbGetAtCommit" {
                                if key != "self-issued-test-key" {
                                    json(StatusCode::UNAUTHORIZED, serde_json::json!({ "code": "unauthenticated", "message": "key required" }))
                                } else if body["table"] == "game_items"
                                    && body["key"] == "player-1"
                                    && body["commitId"] == "commit-abc"
                                {
                                    json(
                                        StatusCode::OK,
                                        serde_json::json!({
                                            "table": "game_items",
                                            "key": "player-1",
                                            "commit_id": "commit-abc",
                                            "value": { "quantity": 3 },
                                        }),
                                    )
                                } else {
                                    json(StatusCode::NOT_FOUND, serde_json::json!({ "code": "not_found", "message": "no value" }))
                                }
                            } else {
                                Response::builder().status(StatusCode::NOT_FOUND).body(Full::new(Bytes::new())).unwrap()
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
