//! Apache `.htaccess`の`RewriteRule`相当のパスリライト/リダイレクト
//! (2026-08-03新設、改善計画「(1) Apache互換の深掘り」対応)。
//!
//! **正直な開示・スコープ**: `mod_rewrite`のディレクティブ体系
//! (`RewriteCond`・`RewriteBase`・`%{HTTP_HOST}`等の変数展開・複雑な
//! フラグの組み合わせ)をフルに再実装するものではない——限定的な
//! サブセットとして、(a) 正規表現パターンによるリクエストパスの
//! マッチング、(b) `$1`/`$2`等のキャプチャグループを使った置換先の
//! 組み立て、(c) 内部リライト(サーバー内部でのパス書き換え、
//! Apacheの`[L]`フラグのみ相当)と外部リダイレクト(実際に`301`+
//! `Location`を返す、Apacheの`[R=301,L]`相当)の2択、のみを実装する。
//! 複数ルールは登録順に評価し、**最初にマッチしたルールで確定**する
//! (Apacheの`[L]`〈これ以上のルール評価を止める〉を常に暗黙的に
//! 適用している設計——`[L]`無しで複数ルールを連鎖させる高度な使い方は
//! 対象外)。
//!
//! **性能上のトレードオフ(正直な開示)**: 現状の実装は`Regex::new`を
//! リクエストのたびに呼ぶ(コンパイル済み正規表現をキャッシュしない)。
//! vhostごとのルール数が少ない一般的な使い方では実用上問題にならない
//! はずだが、非常に多くのルール・高頻度アクセスの本番運用では
//! ボトルネックになりうる——将来、`WebVhostConfig`の変更頻度が低い
//! ことを利用したコンパイル済み正規表現のキャッシュ層を追加する余地が
//! ある(今回のスコープ外、既知の最適化候補として明記)。

use serde::{Deserialize, Serialize};

/// 1件のリライト/リダイレクトルール。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RewriteRule {
    /// リクエストパスに対する正規表現パターン(`regex`クレート構文)。
    pub pattern: String,
    /// マッチした場合の置換先(`$1`/`$2`等のキャプチャグループ参照が
    /// 使える、`regex::Captures::expand`と同じ構文)。
    pub substitution: String,
    /// `true`なら外部リダイレクト(`301 Moved Permanently` +
    /// `Location`ヘッダー、Apacheの`[R=301,L]`相当)。`false`
    /// (既定)なら内部リライト(クライアントには見えず、サーバー内部で
    /// 以後の処理に書き換え後のパスを使う、Apacheの`[L]`のみ相当)。
    #[serde(default)]
    pub redirect: bool,
}

/// リライト適用の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteOutcome {
    /// どのルールにもマッチしなかった(元のパスのまま処理を続ける)。
    Unchanged,
    /// 内部リライト: 以後の処理はこの新しいパスを使う。
    Rewritten(String),
    /// 外部リダイレクト: このURLへ`301`で即座にリダイレクトする。
    Redirect(String),
}

/// `path`に対して`rules`を登録順に評価し、最初にマッチしたルールの
/// 結果を返す(`[L]`を常に暗黙適用、モジュールdoc参照)。不正な正規表現
/// パターンは黙って無視し、以降のルール評価を続ける(1件の設定ミスで
/// vhost全体のリクエスト処理を止めないための安全側の判断)。
pub fn apply(path: &str, rules: &[RewriteRule]) -> RewriteOutcome {
    for rule in rules {
        let re = match regex::Regex::new(&rule.pattern) {
            Ok(re) => re,
            Err(e) => {
                tracing::warn!(pattern = %rule.pattern, error = %e, "skipping rewrite rule with invalid regex pattern");
                continue;
            }
        };
        if let Some(caps) = re.captures(path) {
            let mut result = String::new();
            caps.expand(&rule.substitution, &mut result);
            return if rule.redirect { RewriteOutcome::Redirect(result) } else { RewriteOutcome::Rewritten(result) };
        }
    }
    RewriteOutcome::Unchanged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_rules_leaves_path_unchanged() {
        assert_eq!(apply("/foo/bar", &[]), RewriteOutcome::Unchanged);
    }

    #[test]
    fn unmatched_pattern_leaves_path_unchanged() {
        let rules = vec![RewriteRule { pattern: "^/nomatch$".to_string(), substitution: "/other".to_string(), redirect: false }];
        assert_eq!(apply("/foo/bar", &rules), RewriteOutcome::Unchanged);
    }

    #[test]
    fn internal_rewrite_with_capture_group() {
        // Apacheでよくある「/old/123 を /new.php?id=123 へ内部リライト」パターン。
        let rules = vec![RewriteRule {
            pattern: r"^/old/(\d+)$".to_string(),
            substitution: "/new.php?id=$1".to_string(),
            redirect: false,
        }];
        assert_eq!(apply("/old/123", &rules), RewriteOutcome::Rewritten("/new.php?id=123".to_string()));
    }

    #[test]
    fn external_redirect_returns_redirect_variant() {
        let rules = vec![RewriteRule {
            pattern: r"^/legacy/(.*)$".to_string(),
            substitution: "/modern/$1".to_string(),
            redirect: true,
        }];
        assert_eq!(apply("/legacy/page", &rules), RewriteOutcome::Redirect("/modern/page".to_string()));
    }

    #[test]
    fn first_matching_rule_wins_and_stops_evaluation() {
        let rules = vec![
            RewriteRule { pattern: r"^/a$".to_string(), substitution: "/first".to_string(), redirect: false },
            RewriteRule { pattern: r"^/a$".to_string(), substitution: "/second".to_string(), redirect: false },
        ];
        assert_eq!(apply("/a", &rules), RewriteOutcome::Rewritten("/first".to_string()));
    }

    #[test]
    fn invalid_regex_pattern_is_skipped_not_panicking() {
        let rules = vec![
            RewriteRule { pattern: "(unclosed".to_string(), substitution: "/never".to_string(), redirect: false },
            RewriteRule { pattern: r"^/ok$".to_string(), substitution: "/matched".to_string(), redirect: false },
        ];
        assert_eq!(apply("/ok", &rules), RewriteOutcome::Rewritten("/matched".to_string()));
    }
}
