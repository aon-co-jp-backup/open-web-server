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

/// `RewriteCond`相当の条件付きマッチ(2026-08-05追加、ユーザー指示による
/// vhostフル構文対応の拡張の一部)。
///
/// **正直な開示・スコープ**: Apacheの`RewriteCond`が対応する全ての
/// `%{...}`サーバー変数のうち、実際に評価できるのは以下の限定的な
/// サブセットのみ: `%{HTTP_HOST}`(リクエストのHostヘッダ)・
/// `%{REQUEST_METHOD}`(HTTPメソッド)・`%{QUERY_STRING}`(クエリ文字列)。
/// `%{REMOTE_ADDR}`・`%{HTTPS}`・`%{TIME_*}`等、接続情報やサーバー内部
/// 状態に依存する変数は対象外(このモジュールが受け取る`RewriteContext`
/// が持つ情報の範囲に留めている)。`[OR]`のようなフラグによる複数条件の
/// OR結合も対象外——複数`RewriteCond`は常にAND(Apacheの既定動作)として
/// 評価する。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RewriteCondition {
    /// 評価対象の変数名。`"HTTP_HOST"`/`"REQUEST_METHOD"`/`"QUERY_STRING"`
    /// のいずれか(`%{...}`の中身のみ、波括弧・パーセントは含めない)。
    /// 未知の変数名は常にマッチしない(黙って無視するのではなく、条件を
    /// 満たせないため親の`RewriteRule`全体が不成立になる——安全側の
    /// フェイルクローズ)。
    pub variable: String,
    /// `variable`の実際の値に対する正規表現パターン(`regex`クレート構文)。
    pub pattern: String,
}

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
    /// `RewriteCond`相当の付加条件(2026-08-05追加)。空なら常に
    /// (パスパターンさえ一致すれば)適用される既存動作のまま——既定は
    /// 空ベクタなので既存のTOML/JSON設定・既存テストとの後方互換を
    /// 完全に保つ。複数条件はAND評価(Apacheの`RewriteCond`複数行の
    /// 既定動作と同じ)。
    #[serde(default)]
    pub conditions: Vec<RewriteCondition>,
}

/// リライト評価に必要な、リクエストに関する最小限のコンテキスト
/// (2026-08-05追加)。`RewriteCondition::variable`が参照できる範囲は
/// このコンテキストが保持するフィールドに限られる(スコープの正直な
/// 開示は`RewriteCondition`のdoc参照)。
#[derive(Debug, Clone, Copy, Default)]
pub struct RewriteContext<'a> {
    pub http_host: Option<&'a str>,
    pub request_method: Option<&'a str>,
    pub query_string: Option<&'a str>,
}

impl<'a> RewriteContext<'a> {
    fn resolve_variable(&self, variable: &str) -> Option<&'a str> {
        match variable {
            "HTTP_HOST" => self.http_host,
            "REQUEST_METHOD" => self.request_method,
            "QUERY_STRING" => self.query_string,
            _ => None,
        }
    }
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
///
/// `RewriteCondition`(`RewriteCond`相当)を評価するコンテキストが無い
/// 後方互換の入口。条件付きルール(`conditions`が空でないルール)の場合、
/// コンテキストが無いため条件変数が解決できず、そのルールは常に
/// 不一致として扱われる——条件を実際に評価したい呼び出し元は
/// `apply_with_context`を使うこと。
pub fn apply(path: &str, rules: &[RewriteRule]) -> RewriteOutcome {
    apply_with_context(path, rules, &RewriteContext::default())
}

/// `apply`と同じだが、`RewriteCondition`(`RewriteCond`相当)を実際の
/// リクエスト情報(`ctx`)に対して評価する(2026-08-05追加)。各ルールは
/// (1)`conditions`が全てAND条件で一致し、かつ(2)`pattern`がパスに一致
/// した場合にのみ適用される。`conditions`が空のルールは従来通りパス
/// パターンのみで判定する(完全な後方互換)。
pub fn apply_with_context(path: &str, rules: &[RewriteRule], ctx: &RewriteContext) -> RewriteOutcome {
    for rule in rules {
        if !conditions_match(&rule.conditions, ctx) {
            continue;
        }

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

/// 全ての`conditions`がANDでマッチするか判定する(空なら無条件でtrue)。
/// 未知の変数名・不正な正規表現パターンは「マッチしない」という安全側
/// (フェイルクローズ)の判定にする——条件が誤って常に真になり、意図
/// しないリライト/リダイレクトが発動する事故を避けるため。
fn conditions_match(conditions: &[RewriteCondition], ctx: &RewriteContext) -> bool {
    conditions.iter().all(|cond| {
        let Some(value) = ctx.resolve_variable(&cond.variable) else {
            return false;
        };
        match regex::Regex::new(&cond.pattern) {
            Ok(re) => re.is_match(value),
            Err(e) => {
                tracing::warn!(
                    pattern = %cond.pattern,
                    variable = %cond.variable,
                    error = %e,
                    "skipping rewrite condition with invalid regex pattern"
                );
                false
            }
        }
    })
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
        let rules = vec![RewriteRule { pattern: "^/nomatch$".to_string(), substitution: "/other".to_string(), redirect: false, conditions: vec![] }];
        assert_eq!(apply("/foo/bar", &rules), RewriteOutcome::Unchanged);
    }

    #[test]
    fn internal_rewrite_with_capture_group() {
        // Apacheでよくある「/old/123 を /new.php?id=123 へ内部リライト」パターン。
        let rules = vec![RewriteRule {
            pattern: r"^/old/(\d+)$".to_string(),
            substitution: "/new.php?id=$1".to_string(),
            redirect: false,
            conditions: vec![],
        }];
        assert_eq!(apply("/old/123", &rules), RewriteOutcome::Rewritten("/new.php?id=123".to_string()));
    }

    #[test]
    fn external_redirect_returns_redirect_variant() {
        let rules = vec![RewriteRule {
            pattern: r"^/legacy/(.*)$".to_string(),
            substitution: "/modern/$1".to_string(),
            redirect: true,
            conditions: vec![],
        }];
        assert_eq!(apply("/legacy/page", &rules), RewriteOutcome::Redirect("/modern/page".to_string()));
    }

    #[test]
    fn first_matching_rule_wins_and_stops_evaluation() {
        let rules = vec![
            RewriteRule { pattern: r"^/a$".to_string(), substitution: "/first".to_string(), redirect: false, conditions: vec![] },
            RewriteRule { pattern: r"^/a$".to_string(), substitution: "/second".to_string(), redirect: false, conditions: vec![] },
        ];
        assert_eq!(apply("/a", &rules), RewriteOutcome::Rewritten("/first".to_string()));
    }

    #[test]
    fn invalid_regex_pattern_is_skipped_not_panicking() {
        let rules = vec![
            RewriteRule { pattern: "(unclosed".to_string(), substitution: "/never".to_string(), redirect: false, conditions: vec![] },
            RewriteRule { pattern: r"^/ok$".to_string(), substitution: "/matched".to_string(), redirect: false, conditions: vec![] },
        ];
        assert_eq!(apply("/ok", &rules), RewriteOutcome::Rewritten("/matched".to_string()));
    }

    #[test]
    fn rewrite_condition_on_http_host_gates_the_rule() {
        let rules = vec![RewriteRule {
            pattern: r"^/(.*)$".to_string(),
            substitution: "/mobile/$1".to_string(),
            redirect: false,
            conditions: vec![RewriteCondition {
                variable: "HTTP_HOST".to_string(),
                pattern: r"^m\.example\.com$".to_string(),
            }],
        }];

        let matching_ctx = RewriteContext { http_host: Some("m.example.com"), ..Default::default() };
        assert_eq!(
            apply_with_context("/page", &rules, &matching_ctx),
            RewriteOutcome::Rewritten("/mobile/page".to_string())
        );

        let non_matching_ctx = RewriteContext { http_host: Some("www.example.com"), ..Default::default() };
        assert_eq!(apply_with_context("/page", &rules, &non_matching_ctx), RewriteOutcome::Unchanged);
    }

    #[test]
    fn rewrite_condition_without_context_never_matches_via_plain_apply() {
        // 後方互換の入口`apply`はコンテキストを持たないため、conditions付き
        // ルールはコンテキスト無しでは常に不一致として扱われる。
        let rules = vec![RewriteRule {
            pattern: r"^/(.*)$".to_string(),
            substitution: "/mobile/$1".to_string(),
            redirect: false,
            conditions: vec![RewriteCondition {
                variable: "HTTP_HOST".to_string(),
                pattern: r"^m\.example\.com$".to_string(),
            }],
        }];
        assert_eq!(apply("/page", &rules), RewriteOutcome::Unchanged);
    }

    #[test]
    fn multiple_conditions_are_and_combined() {
        let rules = vec![RewriteRule {
            pattern: r"^/(.*)$".to_string(),
            substitution: "/api/$1".to_string(),
            redirect: false,
            conditions: vec![
                RewriteCondition { variable: "HTTP_HOST".to_string(), pattern: r"^api\.example\.com$".to_string() },
                RewriteCondition { variable: "REQUEST_METHOD".to_string(), pattern: r"^POST$".to_string() },
            ],
        }];

        let both_match = RewriteContext {
            http_host: Some("api.example.com"),
            request_method: Some("POST"),
            ..Default::default()
        };
        assert_eq!(
            apply_with_context("/thing", &rules, &both_match),
            RewriteOutcome::Rewritten("/api/thing".to_string())
        );

        let only_host_matches = RewriteContext {
            http_host: Some("api.example.com"),
            request_method: Some("GET"),
            ..Default::default()
        };
        assert_eq!(apply_with_context("/thing", &rules, &only_host_matches), RewriteOutcome::Unchanged);
    }

    #[test]
    fn unknown_condition_variable_fails_closed() {
        let rules = vec![RewriteRule {
            pattern: r"^/(.*)$".to_string(),
            substitution: "/never/$1".to_string(),
            redirect: false,
            conditions: vec![RewriteCondition { variable: "REMOTE_ADDR".to_string(), pattern: r".*".to_string() }],
        }];
        let ctx = RewriteContext { http_host: Some("example.com"), ..Default::default() };
        assert_eq!(apply_with_context("/thing", &rules, &ctx), RewriteOutcome::Unchanged);
    }
}
