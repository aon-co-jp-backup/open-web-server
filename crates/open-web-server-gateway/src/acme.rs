//! ACME (RFC 8555) HTTP-01チャレンジレスポンダ(2026-07-16、
//! `docs/tls-tenant.md`で次回フェーズ候補として明記していたACME自動取得の
//! 第一歩)。
//!
//! この行程は意図的に2つに分割する:
//! 1. **本ファイル(`ChallengeStore`+ハンドラ)**——ACME CA(Let's Encrypt等)
//!    が公開インターネット経由で実際に接続してくる
//!    `GET /.well-known/acme-challenge/:token`を提供する側。暗号処理・
//!    HTTPクライアント依存は一切無く、小さく自己完結しているため今回
//!    実装する。
//! 2. **ACMEクライアント本体(ディレクトリ探索・nonce管理・JWS署名・
//!    account/order/challenge/finalizeステートマシン)は今回スコープ外**
//!    ——`poem-cosmo-tauri`の`crates/open-runo-router/src/acme.rs`
//!    (`#[cfg(feature = "acme")] mod client`)に既に実装・テスト済みだが、
//!    `open_runo_core::{AppError, Result}`・`crate::hyper_compat::
//!    {Handler, Params}`というpoem-cosmo-tauri固有の型に深く結合した
//!    ~1500行超のコードであり、このリポジトリの別の型体系
//!    (`response::BoxBody`等)へ機械的に移植するには1パスでは検証しきれない
//!    規模と判断した。次回セッションで、型を1つずつ対応させながら
//!    移植することを推奨する(このファイルの`ChallengeStore`は移植先が
//!    決まった時点でそのまま流用できる設計にしてある)。

use std::collections::HashMap;
use std::sync::Mutex;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};

use crate::response::BoxBody;

/// トークン → key-authorization のインメモリ対応表。
/// `open-web-server`自体が公開ACMEクライアントを実装していなくても、
/// 外部のACMEクライアント(certbot等)がこのプロセスに向けて発行した
/// チャレンジをそのまま配信できるよう、常時コンパイルする(暗号/HTTP
/// クライアント依存が無いため軽量)。
#[derive(Debug, Default)]
pub struct ChallengeStore {
    tokens: Mutex<HashMap<String, String>>,
}

impl ChallengeStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&self, token: String, key_authorization: String) {
        self.tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(token, key_authorization);
    }

    pub fn get(&self, token: &str) -> Option<String> {
        self.tokens.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(token).cloned()
    }

    pub fn remove(&self, token: &str) {
        self.tokens.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(token);
    }
}

/// `GET /.well-known/acme-challenge/:token` — ACME CAのHTTP-01検証が
/// 実際に接続してくるエンドポイント。公開済みのkey authorizationを
/// `text/plain`で返す(RFC 8555 §8.3)。未公開/期限切れ/既に削除済みの
/// トークンは404。
pub async fn challenge_response_handler(store: &ChallengeStore, req: &Request<Incoming>) -> Response<BoxBody> {
    let path = req.uri().path();
    let Some(token) = path.strip_prefix("/.well-known/acme-challenge/") else {
        return not_found();
    };
    if token.is_empty() {
        return not_found();
    }
    match store.get(token) {
        Some(key_auth) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain")
            .body(BoxBody::from(key_auth))
            .expect("building a response from a fixed set of valid headers cannot fail"),
        None => not_found(),
    }
}

fn not_found() -> Response<BoxBody> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(BoxBody::from(String::new()))
        .expect("building a response from a fixed set of valid headers cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `challenge_response_handler`は`Request<Incoming>`(実TCP接続からしか
    /// 構築できない型)の`uri().path()`しか読まないため、パス抽出ロジック
    /// だけを切り出してユニットテストする(ハンドラ自体のエンドツーエンド
    /// 検証は実サーバー起動を伴う統合テストの領分——このモジュールの
    /// スコープは「正しいパスをパースするか」に絞る)。
    fn extract_token(path: &str) -> Option<&str> {
        let token = path.strip_prefix("/.well-known/acme-challenge/")?;
        if token.is_empty() {
            None
        } else {
            Some(token)
        }
    }

    #[test]
    fn extracts_token_from_well_known_path() {
        assert_eq!(extract_token("/.well-known/acme-challenge/abc123"), Some("abc123"));
    }

    #[test]
    fn rejects_empty_token_and_unrelated_paths() {
        assert_eq!(extract_token("/.well-known/acme-challenge/"), None);
        assert_eq!(extract_token("/healthz"), None);
    }

    #[test]
    fn publish_get_remove_round_trip() {
        let store = ChallengeStore::new();
        assert_eq!(store.get("token-a"), None);
        store.publish("token-a".to_string(), "key-auth-a".to_string());
        assert_eq!(store.get("token-a"), Some("key-auth-a".to_string()));
        store.remove("token-a");
        assert_eq!(store.get("token-a"), None);
        // Removing again (already absent) must not panic -- idempotent.
        store.remove("token-a");
    }
}
