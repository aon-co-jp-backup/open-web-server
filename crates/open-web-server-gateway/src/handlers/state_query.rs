//! `Connect Admin/GetStateAtCommit` — VersionLessAPI +
//! Git-on-SQL ハイブリッドの読み出し側(拡張要件(1))。
//!
//! これまで拡張要件(1)は`MutationReceipt.db_commit_id`の配線という
//! 書き込み側のみ実質的に完成しており、「commit_idを指定して過去状態を
//! 問い合わせる」読み出し側は`open-web-server`に一切存在しなかった
//! (open-runo側の`GET /api/db/:table/:key/at/:commit_id`は
//! 2026-07-13に実装・検証済みだったが、こちら側に対応するエンドポイントが
//! 無く接続されていなかった)。このハンドラは`open-web-server-ledger::
//! DbStateReader`を通じてopen-runoのそのエンドポイントへプロキシし、
//! ギャップを閉じる。

use std::sync::Arc;

use bytes::Bytes;
use hyper::{Request, Response, StatusCode};

use crate::response::{json_response, text_response, BoxBody};
use crate::state::AppState;

struct ParsedPath {
    target: String,
    key: String,
    commit_id: String,
}

/// `Connect Admin/GetStateAtCommit` の本体。
///
/// **2026-07-30追記(セキュリティ強化)**: 過去のコミット時点の状態
/// (課金アイテム・金融/決済データを含みうる)を誰でも無認証で照会
/// できてしまっていた実際のギャップ(`aruaru-server`の`/admin/*`で
/// 発見した「非公開APIが無認証」と同種のパターン)を修正——`/api/v1/*`
/// (公開型の決済API)とは異なり、この`/internal/*`エンドポイントは
/// 内部の状態照会用途であり公開すべきではないため、既存の
/// `handlers::tenants::check_admin_auth`(`x-admin-token`/
/// `KeyGuardian`認証)を必須にした。
#[tracing::instrument(
    name = "get_state_at_commit",
    skip(state, req),
    fields(target = tracing::field::Empty, key = tracing::field::Empty, commit_id = tracing::field::Empty)
)]
pub async fn get_state_at_commit(state: Arc<AppState>, req: &Request<Bytes>, target: &str, key: &str, commit_id: &str) -> Response<BoxBody> {
    if let Err(resp) = crate::handlers::tenants::check_admin_auth(&state, req) {
        return resp;
    }

    if target.is_empty() || key.is_empty() || commit_id.is_empty() {
        return text_response(StatusCode::BAD_REQUEST, "'target', 'key' and 'commit_id' must be non-empty");
    }
    let parsed = ParsedPath { target: target.to_string(), key: key.to_string(), commit_id: commit_id.to_string() };

    let span = tracing::Span::current();
    span.record("target", parsed.target.as_str());
    span.record("key", parsed.key.as_str());
    span.record("commit_id", parsed.commit_id.as_str());

    match state
        .db_state_reader
        .get_at_commit(&parsed.target, &parsed.key, &parsed.commit_id)
        .await
    {
        Ok(Some(response)) => json_response(StatusCode::OK, &response),
        Ok(None) => text_response(
            StatusCode::NOT_FOUND,
            format!(
                "no value for {}/{} as of commit {} \
                 (commit unknown, or the key did not exist yet at that point)",
                parsed.target, parsed.key, parsed.commit_id
            ),
        ),
        // open-runo自体が到達不能・想定外ステータスを返した場合は
        // 502(このゲートウェイの下流障害)を返す——404(このゲートウェイ
        // 自体にリソースが無い)とは区別する。
        Err(e) => text_response(StatusCode::BAD_GATEWAY, format!("open-runo request failed: {e}")),
    }
}
