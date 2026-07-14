//! `GET /internal/db/state/:target/:key/at/:commit_id` — VersionLessAPI +
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

use hyper::{Response, StatusCode};

use crate::response::{json_response, text_response, BoxBody};
use crate::state::AppState;

struct ParsedPath {
    target: String,
    key: String,
    commit_id: String,
}

/// `/internal/db/state/:target/:key/at/:commit_id` をパースする。
/// フレームワーク非依存の自前ディスパッチャ(`main.rs`の`dispatch`)は
/// 動的パスパラメータを持たないため、ここで手動パースする。
fn parse_path(path: &str) -> Option<ParsedPath> {
    let rest = path.strip_prefix("/internal/db/state/")?;
    let segments: Vec<&str> = rest.split('/').collect();
    let [target, key, "at", commit_id] = segments[..] else {
        return None;
    };
    if target.is_empty() || key.is_empty() || commit_id.is_empty() {
        return None;
    }
    Some(ParsedPath {
        target: target.to_string(),
        key: key.to_string(),
        commit_id: commit_id.to_string(),
    })
}

/// `GET /internal/db/state/:target/:key/at/:commit_id` の本体。
#[tracing::instrument(
    name = "get_state_at_commit",
    skip(state),
    fields(target = tracing::field::Empty, key = tracing::field::Empty, commit_id = tracing::field::Empty)
)]
pub async fn get_state_at_commit(state: Arc<AppState>, path: &str) -> Response<BoxBody> {
    let Some(parsed) = parse_path(path) else {
        return text_response(
            StatusCode::BAD_REQUEST,
            "expected /internal/db/state/:target/:key/at/:commit_id",
        );
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_path_extracts_target_key_and_commit_id() {
        let parsed = parse_path("/internal/db/state/game_items/player-1/at/commit-abc").unwrap();
        assert_eq!(parsed.target, "game_items");
        assert_eq!(parsed.key, "player-1");
        assert_eq!(parsed.commit_id, "commit-abc");
    }

    #[test]
    fn parse_path_rejects_missing_at_segment() {
        assert!(parse_path("/internal/db/state/game_items/player-1/commit-abc").is_none());
    }

    #[test]
    fn parse_path_rejects_wrong_segment_count() {
        assert!(parse_path("/internal/db/state/game_items/at/commit-abc").is_none());
        assert!(parse_path("/internal/db/state/game_items/player-1/at/commit-abc/extra").is_none());
    }

    #[test]
    fn parse_path_rejects_empty_segments() {
        assert!(parse_path("/internal/db/state//player-1/at/commit-abc").is_none());
        assert!(parse_path("/internal/db/state/game_items/player-1/at/").is_none());
    }
}
