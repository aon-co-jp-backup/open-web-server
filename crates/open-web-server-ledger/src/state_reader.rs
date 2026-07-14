//! VersionLessAPI + Git-on-SQL ハイブリッドの読み出し側 (拡張要件(1))。
//!
//! `Ledger::commit`(書き込み側、`forward_once`経由でopen-runoへ確定を
//! フォワードする)の対になる、履歴読み出し側。ある target/key が指定
//! コミット時点で保持していた値を、open-runoの
//! `GET /api/db/:table/:key/at/:commit_id`(2026-07-13実装済み、
//! `AruaruDbBackend`のみ実対応で他バックエンドは501)へプロキシして返す。
//! `open-web-server-gateway`側の
//! `GET /internal/db/state/:target/:key/at/:commit_id`ハンドラから使われる
//! (拡張要件(1)が長らく「書き込み側のみ実装済み」だったギャップを解消)。
//!
//! **認証**: open-runoのこのエンドポイントは`X-Api-Key`(またはセッション
//! Cookie)を要求する。運用者が固定キーを管理・ローテーションする代わりに、
//! `open-runo-cli`・WASMフロントエンドが既に採用している「人間がAPIキーを
//! 意識しない」方針をここでも踏襲する: 初回利用時に`POST
//! /api/keys/self-issue`で短命な developer role キーを自動発行しキャッシュ、
//! `401`が返ってきた場合(キャッシュ済みキーの期限切れ)は透過的に
//! 再発行してリトライする。

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
        let resp = self.request(target, key, commit_id, &api_key).await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            // キャッシュ済みキーが期限切れ(self-issueキーは短命)——
            // 一度だけ再発行してリトライする。呼び出し元には
            // 「たまたま期限が切れていた」という実装詳細を露出しない。
            let fresh_key = self.self_issue_key().await?;
            *self.cached_api_key.lock().await = Some(fresh_key.clone());
            let resp = self.request(target, key, commit_id, &fresh_key).await?;
            return Self::parse_response(target, key, commit_id, resp).await;
        }

        Self::parse_response(target, key, commit_id, resp).await
    }

    async fn request(
        &self,
        target: &str,
        key: &str,
        commit_id: &str,
        api_key: &str,
    ) -> anyhow::Result<reqwest::Response> {
        let url = format!("{}/api/db/{target}/{key}/at/{commit_id}", self.open_runo_endpoint);
        self.http
            .get(url)
            .header("x-api-key", api_key)
            .send()
            .await
            .map_err(Into::into)
    }

    async fn parse_response(
        target: &str,
        key: &str,
        commit_id: &str,
        resp: reqwest::Response,
    ) -> anyhow::Result<Option<DbStateAtCommitResponse>> {
        match resp.status() {
            reqwest::StatusCode::OK => {
                let body: serde_json::Value = resp.json().await?;
                let value = body.get("value").cloned().unwrap_or(serde_json::Value::Null);
                Ok(Some(DbStateAtCommitResponse {
                    target: target.to_string(),
                    key: key.to_string(),
                    commit_id: commit_id.to_string(),
                    value,
                }))
            }
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            // 501 (バックエンドがコミット履歴クエリ自体に未対応) を含め、
            // それ以外の全ステータスは呼び出し元にエラーとして伝える。
            other => Err(anyhow::anyhow!(
                "open-runo returned unexpected status {other} for {target}/{key}@{commit_id}"
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
        let url = format!("{}/api/keys/self-issue", self.open_runo_endpoint);
        let body: serde_json::Value = self
            .http
            .post(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        body.get("api_key")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("open-runo self-issue response missing api_key"))
    }
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

    /// 最小限のopen-runoモック: `/api/keys/self-issue`(呼び出し回数を
    /// 記録)と`/api/db/:table/:key/at/:commit_id`(固定のtarget/key/
    /// commit_idにのみ200を返し、それ以外は404)。実TCP+実HTTPで
    /// `DbStateReader`を検証するための最小サーバー。
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
                            let path = req.uri().path().to_string();
                            let method = req.method().clone();
                            let resp = if method == Method::POST && path == "/api/keys/self-issue" {
                                calls.fetch_add(1, Ordering::SeqCst);
                                Response::builder()
                                    .status(StatusCode::OK)
                                    .header("content-type", "application/json")
                                    .body(Full::new(Bytes::from(
                                        serde_json::json!({ "api_key": "self-issued-test-key", "expires_at": "2099-01-01T00:00:00Z" })
                                            .to_string(),
                                    )))
                                    .unwrap()
                            } else if method == Method::GET
                                && path == "/api/db/game_items/player-1/at/commit-abc"
                            {
                                Response::builder()
                                    .status(StatusCode::OK)
                                    .header("content-type", "application/json")
                                    .body(Full::new(Bytes::from(
                                        serde_json::json!({
                                            "table": "game_items",
                                            "key": "player-1",
                                            "commit_id": "commit-abc",
                                            "value": { "quantity": 3 },
                                        })
                                        .to_string(),
                                    )))
                                    .unwrap()
                            } else {
                                Response::builder()
                                    .status(StatusCode::NOT_FOUND)
                                    .body(Full::new(Bytes::new()))
                                    .unwrap()
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
}
