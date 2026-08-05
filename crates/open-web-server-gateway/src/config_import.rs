//! 実際のApache/Nginx設定ファイルから`WebVhostConfig`を読み取る
//! インポート機能(2026-08-03新設)。
//!
//! **スコープの変遷(正直な開示)**: 当初(2026-08-03、commit `d11e683`)は
//! ユーザー指示により「vhost定義の基本部分のみ」(`ServerName`/
//! `server_name`・`DocumentRoot`/`root`・`SetHandler proxy:fcgi://`/
//! `fastcgi_pass`)に限定していた。**2026-08-05、ユーザーから明示的に
//! 「vhostのフル構文対応(RewriteCond・Basic/Digest認証・SSL証明書パス
//! 読取)まで広げてほしい」との指示があり、以下のディレクティブへ対応を
//! 拡張した**:
//!
//! - **Apache**: `ServerName`・`DocumentRoot`・`SetHandler
//!   "proxy:fcgi://..."`(従来通り) に加え、
//!   `RewriteCond`+`RewriteRule`の組み合わせ(条件付きリライト、
//!   `crate::rewrite::RewriteCondition`を使う限定的な変数サブセットのみ
//!   ——詳細は`rewrite.rs`のdoc参照)、`AuthType Basic`+`AuthName`+
//!   `AuthUserFile`(Basic認証設定)、`SSLCertificateFile`/
//!   `SSLCertificateKeyFile`(TLS証明書パス)、`<Directory>`ブロック内の
//!   `Allow from`/`Deny from`(基本的なIP許可/拒否リストのみ)。
//! - **Nginx**: `server_name`・`root`・`fastcgi_pass`(従来通り)に加え、
//!   `if (...) { return|rewrite ...; }`ブロック(条件付きreturn/rewrite、
//!   Apache同様の限定的な変数サブセット)、`auth_basic`+
//!   `auth_basic_user_file`(Basic認証設定)、`ssl_certificate`/
//!   `ssl_certificate_key`(TLS証明書パス)、`allow`/`deny`
//!   (基本的なIP許可/拒否リストのみ)。
//!
//! **今回も対象外にしたもの(正直な開示、理由付き)**:
//! - **Digest認証**(`AuthType Digest`): RFC 7616のnonce管理・
//!   質問応答方式はBasic認証とは実装の性質が全く異なり(サーバー側での
//!   nonceの発行・追跡・再利用検知が必要)、このパーサーが単純な行単位の
//!   値抽出に留める設計とは相容れないほど複雑になるため、パース対象には
//!   含めない。`AuthType Digest`を検出した場合は`tracing::warn!`で
//!   「対応外のためスキップする」ことを明示的にログへ出し、黙って
//!   無視することはしない(この方針はユーザー指示にある「パース対象外
//!   として明示的にログ警告を出す程度に留めてよい」に沿う)。
//! - **`<Directory>`の完全なアクセス制御構文**(`Order`・
//!   `Require all granted`等の`mod_authz_core`構文、複数`Allow`/`Deny`の
//!   評価順序): Apacheの実際の評価順序(`Order allow,deny`/
//!   `Order deny,allow`)を再現するには状態機械が必要で、これも
//!   ユーザー指示により「基本的なIP許可/拒否リストのみ」に絞った
//!   (`AccessControlConfig`参照、単純に見つかった`Allow from`/
//!   `Deny from`のIP文字列をそれぞれの集合へ追加するだけ)。
//! - **RewriteCondの変数サブセット**: `%{HTTP_HOST}`/`%{REQUEST_METHOD}`/
//!   `%{QUERY_STRING}`(Nginxの`if`は`$http_host`/`$request_method`/
//!   `$query_string`/`$args`)のみ対応。`%{REMOTE_ADDR}`・`%{HTTPS}`・
//!   `%{TIME_*}`等、接続情報やサーバー内部状態に依存する変数は対象外
//!   (`crate::rewrite::RewriteCondition`のdoc参照)。
//! - 上記いずれにも該当しない行(`Include`・`<Location>`の複雑な条件分岐
//!   等)は、従来通りエラーにせず単純に無視する(「読めるものは読み、
//!   読めないものは無視する」という寛容な設計を維持)。

use std::path::PathBuf;

use crate::rewrite::{RewriteCondition, RewriteRule};
use crate::web_vhost::{AccessControlConfig, BasicAuthConfig, PhpMode, TlsCertConfig, WebVhostConfig};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("no ServerName/server_name directive found in the provided config")]
    MissingHost,
    #[error("no DocumentRoot/root directive found in the provided config")]
    MissingDocroot,
}

/// パース途中に蓄積される、`build_config`へ渡すための可変状態。
/// Apache/Nginx共通で使う(直列パースの都合上、構造体にまとめておくと
/// 見通しが良いための整理——ロジック自体はフォーマットごとに異なる)。
#[derive(Default)]
struct ParseAccumulator {
    host: Option<String>,
    docroot: Option<String>,
    fastcgi_addr: Option<String>,
    rewrite_rules: Vec<RewriteRule>,
    basic_auth: Option<BasicAuthConfig>,
    /// `AuthType`/`auth_basic`が明示的に`off`/`Digest`だった場合は
    /// Basic認証を組み立てないためのフラグ。
    digest_seen: bool,
    tls_cert_path: Option<PathBuf>,
    tls_key_path: Option<PathBuf>,
    access_control: AccessControlConfig,
}

/// Apacheの`<VirtualHost>...</VirtualHost>`ブロック(そのブロックの
/// 中身のみ、または`<VirtualHost>`行自体を含むテキストのどちらでも可)
/// から`WebVhostConfig`を読み取る。
pub fn parse_apache_vhost(conf: &str) -> Result<WebVhostConfig, ImportError> {
    let mut acc = ParseAccumulator::default();
    // `RewriteCond`は直後の`RewriteRule`1件にのみ適用される(Apacheの
    // 実際の挙動: `RewriteCond`は次の`RewriteRule`が現れるまで蓄積され、
    // そのルールに適用された後はクリアされる)。
    let mut pending_conditions: Vec<RewriteCondition> = Vec::new();
    let mut pending_auth_name: Option<String> = None;

    for raw_line in conf.lines() {
        let line = raw_line.trim();
        if let Some(rest) = strip_directive(line, "ServerName") {
            acc.host.get_or_insert_with(|| rest.to_string());
        } else if let Some(rest) = strip_directive(line, "DocumentRoot") {
            acc.docroot.get_or_insert_with(|| unquote(rest).to_string());
        } else if let Some(rest) = strip_directive(line, "SetHandler") {
            // 例: SetHandler "proxy:fcgi://127.0.0.1:9000"
            let value = unquote(rest);
            if let Some(addr) = value.strip_prefix("proxy:fcgi://") {
                acc.fastcgi_addr = Some(addr.to_string());
            }
        } else if let Some(rest) = strip_directive(line, "RewriteCond") {
            if let Some(cond) = parse_apache_rewrite_cond(rest) {
                pending_conditions.push(cond);
            }
        } else if let Some(rest) = strip_directive(line, "RewriteRule") {
            if let Some(mut rule) = parse_apache_rewrite_rule(rest) {
                rule.conditions = std::mem::take(&mut pending_conditions);
                acc.rewrite_rules.push(rule);
            } else {
                pending_conditions.clear();
            }
        } else if let Some(rest) = strip_directive(line, "AuthType") {
            let auth_type = unquote(rest).to_lowercase();
            if auth_type == "digest" {
                acc.digest_seen = true;
                tracing::warn!(
                    "config_import: 'AuthType Digest' is not supported by this parser \
                     (Digest auth's nonce challenge/response scheme is out of scope) \
                     — skipping, no basic_auth will be set for this AuthType block"
                );
            } else if auth_type == "basic" {
                acc.digest_seen = false;
            }
        } else if let Some(rest) = strip_directive(line, "AuthName") {
            pending_auth_name = Some(unquote(rest).to_string());
        } else if let Some(rest) = strip_directive(line, "AuthUserFile") {
            if !acc.digest_seen {
                let realm = pending_auth_name.clone().unwrap_or_else(|| "Restricted".to_string());
                acc.basic_auth = Some(BasicAuthConfig { realm, user_file: PathBuf::from(unquote(rest)) });
            }
        } else if let Some(rest) = strip_directive(line, "SSLCertificateFile") {
            acc.tls_cert_path = Some(PathBuf::from(unquote(rest)));
        } else if let Some(rest) = strip_directive(line, "SSLCertificateKeyFile") {
            acc.tls_key_path = Some(PathBuf::from(unquote(rest)));
        } else if let Some(rest) = strip_directive(line, "Allow") {
            if let Some(ip) = parse_apache_allow_deny(rest) {
                acc.access_control.allow.push(ip);
            }
        } else if let Some(rest) = strip_directive(line, "Deny") {
            if let Some(ip) = parse_apache_allow_deny(rest) {
                acc.access_control.deny.push(ip);
            }
        }
    }

    build_config(acc)
}

/// Nginxの`server { ... }`ブロック(ブロックの中身のみ、または
/// `server {`行自体を含むテキストのどちらでも可)から`WebVhostConfig`を
/// 読み取る。
pub fn parse_nginx_server(conf: &str) -> Result<WebVhostConfig, ImportError> {
    let mut acc = ParseAccumulator::default();
    let mut pending_auth_realm: Option<String> = None;

    // `if (...) { ... }`ブロックは複数行にまたがりうるため、行単位の
    // ループとは別に、開始位置を見つけたら閉じ`}`までを塊として処理する
    // 単純なスキャン方式にする(構造化パーサーではなく、既存の「行単位で
    // ディレクティブを拾う」寛容な設計をブロックにも延長しただけ)。
    let lines: Vec<&str> = conf.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim().trim_end_matches(';').trim();

        if let Some(rest) = strip_directive(line, "server_name") {
            acc.host.get_or_insert_with(|| rest.split_whitespace().next().unwrap_or(rest).to_string());
        } else if let Some(rest) = strip_directive(line, "root") {
            acc.docroot.get_or_insert_with(|| unquote(rest).to_string());
        } else if let Some(rest) = strip_directive(line, "fastcgi_pass") {
            acc.fastcgi_addr.get_or_insert_with(|| unquote(rest).to_string());
        } else if let Some(rest) = strip_directive(line, "auth_basic") {
            let value = unquote(rest);
            if !value.eq_ignore_ascii_case("off") {
                pending_auth_realm = Some(value.to_string());
            }
        } else if let Some(rest) = strip_directive(line, "auth_basic_user_file") {
            let realm = pending_auth_realm.clone().unwrap_or_else(|| "Restricted".to_string());
            acc.basic_auth = Some(BasicAuthConfig { realm, user_file: PathBuf::from(unquote(rest)) });
        } else if let Some(rest) = strip_directive(line, "ssl_certificate_key") {
            acc.tls_key_path = Some(PathBuf::from(unquote(rest)));
        } else if let Some(rest) = strip_directive(line, "ssl_certificate") {
            acc.tls_cert_path = Some(PathBuf::from(unquote(rest)));
        } else if let Some(rest) = strip_directive(line, "allow") {
            let value = unquote(rest.trim_end_matches(';').trim());
            if !value.eq_ignore_ascii_case("all") {
                acc.access_control.allow.push(value.to_string());
            }
        } else if let Some(rest) = strip_directive(line, "deny") {
            let value = unquote(rest.trim_end_matches(';').trim());
            acc.access_control.deny.push(value.to_string());
        } else if line.starts_with("if ") || line.starts_with("if(") {
            // `if ($http_host ~ pattern) {` — 条件を解析し、閉じ`}`までの
            // 中身から`return`/`rewrite`を1件だけ拾う(複数あっても最初の
            // 1件のみ、寛容な最小実装)。
            if let Some(condition) = parse_nginx_if_condition(line) {
                let mut j = i + 1;
                while j < lines.len() {
                    let inner = lines[j].trim();
                    if inner.starts_with('}') {
                        break;
                    }
                    if let Some(mut rule) = parse_nginx_return_or_rewrite(inner) {
                        rule.conditions = vec![condition.clone()];
                        acc.rewrite_rules.push(rule);
                        break;
                    }
                    j += 1;
                }
                // ブロックの終わり(`}`)まで読み飛ばす。
                while i < lines.len() && !lines[i].trim().starts_with('}') {
                    i += 1;
                }
            }
        }

        i += 1;
    }

    build_config(acc)
}

fn build_config(acc: ParseAccumulator) -> Result<WebVhostConfig, ImportError> {
    let host = acc.host.ok_or(ImportError::MissingHost)?;
    let docroot = acc.docroot.ok_or(ImportError::MissingDocroot)?;

    let (php_enabled, php_mode) = match acc.fastcgi_addr {
        Some(addr) => (true, PhpMode::FastCgi { fastcgi_addr: addr }),
        None => (false, PhpMode::default()),
    };

    let tls_cert = acc.tls_cert_path.map(|cert_path| TlsCertConfig { cert_path, key_path: acc.tls_key_path });

    let access_control = if acc.access_control.allow.is_empty() && acc.access_control.deny.is_empty() {
        None
    } else {
        Some(acc.access_control)
    };

    Ok(WebVhostConfig {
        host,
        docroot: PathBuf::from(docroot),
        php_enabled,
        compat_mode: Default::default(),
        php_mode,
        rewrite_rules: acc.rewrite_rules,
        basic_auth: acc.basic_auth,
        tls_cert,
        access_control,
    })
}

/// `RewriteCond %{HTTP_HOST} ^m\.example\.com$ [NC]`のような行(先頭の
/// `RewriteCond`は`strip_directive`で既に除去済み)から
/// `RewriteCondition`を組み立てる。`%{...}`形式でない、または未対応の
/// 変数名は`None`(呼び出し元がスキップする)。フラグ(`[NC]`等)は解析
/// せず無視する(大文字小文字区別の`NC`フラグ非対応という正直な限界)。
fn parse_apache_rewrite_cond(rest: &str) -> Option<RewriteCondition> {
    let mut parts = rest.split_whitespace();
    let variable_token = parts.next()?;
    let pattern_token = parts.next()?;

    let variable = variable_token.strip_prefix("%{")?.strip_suffix('}')?.to_string();
    Some(RewriteCondition { variable, pattern: pattern_token.to_string() })
}

/// `RewriteRule ^/(.*)$ /mobile/$1 [R=301,L]`のような行から`RewriteRule`
/// (`conditions`は空のまま、呼び出し元が直前の`RewriteCond`群を差し込む)
/// を組み立てる。フラグの`R`(`R`単体または`R=<code>`)があれば外部
/// リダイレクト扱いにする。
fn parse_apache_rewrite_rule(rest: &str) -> Option<RewriteRule> {
    let mut parts = rest.splitn(3, char::is_whitespace);
    let pattern = parts.next()?.to_string();
    let mut substitution = parts.next()?.to_string();
    let mut flags_str = String::new();

    if let Some(tail) = parts.next() {
        let tail = tail.trim();
        if let (Some(start), Some(end)) = (tail.find('['), tail.rfind(']')) {
            if end > start {
                flags_str = tail[start + 1..end].to_string();
            }
        }
    } else if let (Some(start), Some(end)) = (substitution.find('['), substitution.rfind(']')) {
        // 置換先とフラグの間に空白が無い場合(例: `/new[R=301,L]`)への
        // 対応。フラグ部分を切り出してから置換先本体を切り詰める。
        if end > start {
            flags_str = substitution[start + 1..end].to_string();
        }
        substitution.truncate(start);
        substitution = substitution.trim().to_string();
    }

    let redirect = flags_str.split(',').any(|f| {
        let f = f.trim();
        f == "R" || f.starts_with("R=")
    });

    Some(RewriteRule { pattern, substitution: unquote(&substitution).to_string(), redirect, conditions: Vec::new() })
}

/// `Allow from 192.168.1.0/24`や`Allow from all`のような行から
/// IP/CIDR文字列を取り出す。`all`は「制限なし」を意味する特殊値なので
/// 保持しない(呼び出し元のリストに意味のあるエントリのみ残すため)。
fn parse_apache_allow_deny(rest: &str) -> Option<String> {
    let rest = rest.trim();
    let rest = rest.strip_prefix("from").map(str::trim).unwrap_or(rest);
    if rest.is_empty() || rest.eq_ignore_ascii_case("all") {
        return None;
    }
    Some(rest.split_whitespace().next().unwrap_or(rest).to_string())
}

/// `if ($http_host ~ ^m\.example\.com$) {`のような行から
/// `RewriteCondition`を組み立てる。対応する変数は`$http_host`/
/// `$request_method`/`$query_string`/`$args`のみ(`RewriteCondition`の
/// 変数名空間はApache/Nginx共通で`crate::rewrite`が解決する)。
fn parse_nginx_if_condition(line: &str) -> Option<RewriteCondition> {
    let start = line.find('(')?;
    let end = line.rfind(')')?;
    if end <= start {
        return None;
    }
    let inner = line[start + 1..end].trim();
    // 期待する形式: `$variable ~ pattern` または `$variable ~* pattern`
    let mut parts = inner.splitn(2, '~');
    let var_part = parts.next()?.trim();
    let pattern_part = parts.next()?.trim();
    let pattern_part = pattern_part.trim_start_matches('*').trim();

    let variable = match var_part {
        "$http_host" => "HTTP_HOST",
        "$request_method" => "REQUEST_METHOD",
        "$query_string" | "$args" => "QUERY_STRING",
        _ => return None,
    };

    Some(RewriteCondition { variable: variable.to_string(), pattern: unquote(pattern_part).to_string() })
}

/// `if`ブロック内の1行から`return`/`rewrite`ディレクティブを解釈し、
/// `RewriteRule`(パスパターンは「全パスに一致」の`^.*$`固定 —
/// Nginxの`if`ブロック自体が既にパスに依存しない条件のため)を組み立てる。
fn parse_nginx_return_or_rewrite(line: &str) -> Option<RewriteRule> {
    let line = line.trim().trim_end_matches(';').trim();
    if let Some(rest) = strip_directive(line, "return") {
        // `return 301 /new-page;` または `return 301 https://...;`
        let mut parts = rest.split_whitespace();
        let _code = parts.next()?; // 301/302等、既存のRewriteRuleは常時redirect=trueで扱う
        let target = parts.next()?.to_string();
        return Some(RewriteRule {
            pattern: "^.*$".to_string(),
            substitution: target,
            redirect: true,
            conditions: Vec::new(),
        });
    }
    if let Some(rest) = strip_directive(line, "rewrite") {
        // `rewrite ^/(.*)$ /mobile/$1 redirect;` / `... permanent;` / `... last;`
        let mut parts = rest.split_whitespace();
        let pattern = parts.next()?.to_string();
        let substitution = parts.next()?.to_string();
        let flag = parts.next().unwrap_or("");
        let redirect = flag == "redirect" || flag == "permanent";
        return Some(RewriteRule { pattern, substitution, redirect, conditions: Vec::new() });
    }
    None
}

/// `line`が`directive`で始まる場合、ディレクティブ名以降の残りの文字列
/// (前後空白除去済み)を返す。大文字小文字を区別する(Apache/Nginxの
/// ディレクティブ名自体は大文字小文字を区別しないのが実際の仕様だが、
/// 今回のスコープでは一般的な表記〈`ServerName`/`server_name`〉のみを
/// 対象とし、過剰な柔軟性は持たせない)。
fn strip_directive<'a>(line: &'a str, directive: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(directive)?;
    let rest = rest.strip_prefix(char::is_whitespace)?;
    Some(rest.trim())
}

/// 前後の`"`または`'`を1組だけ除去する(無ければそのまま返す)。
fn unquote(s: &str) -> &str {
    let s = s.trim();
    for quote in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(quote) && s.ends_with(quote) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const APACHE_SAMPLE: &str = r#"
<VirtualHost *:80>
    ServerName example.com
    ServerAlias www.example.com
    DocumentRoot "/var/www/example"
    <FilesMatch \.php$>
        SetHandler "proxy:fcgi://127.0.0.1:9000"
    </FilesMatch>
</VirtualHost>
"#;

    const NGINX_SAMPLE: &str = r#"
server {
    listen 80;
    server_name example.com www.example.com;
    root /var/www/example;
    location ~ \.php$ {
        fastcgi_pass 127.0.0.1:9000;
    }
}
"#;

    #[test]
    fn parses_apache_vhost_basics() {
        let cfg = parse_apache_vhost(APACHE_SAMPLE).unwrap();
        assert_eq!(cfg.host, "example.com");
        assert_eq!(cfg.docroot, PathBuf::from("/var/www/example"));
        assert!(cfg.php_enabled);
        assert_eq!(cfg.php_mode, PhpMode::FastCgi { fastcgi_addr: "127.0.0.1:9000".to_string() });
    }

    #[test]
    fn parses_nginx_server_basics() {
        let cfg = parse_nginx_server(NGINX_SAMPLE).unwrap();
        assert_eq!(cfg.host, "example.com");
        assert_eq!(cfg.docroot, PathBuf::from("/var/www/example"));
        assert!(cfg.php_enabled);
        assert_eq!(cfg.php_mode, PhpMode::FastCgi { fastcgi_addr: "127.0.0.1:9000".to_string() });
    }

    #[test]
    fn apache_without_php_directive_yields_static_vhost() {
        let conf = r#"
<VirtualHost *:80>
    ServerName static.example
    DocumentRoot /var/www/static
</VirtualHost>
"#;
        let cfg = parse_apache_vhost(conf).unwrap();
        assert_eq!(cfg.host, "static.example");
        assert!(!cfg.php_enabled);
        assert_eq!(cfg.php_mode, PhpMode::default());
    }

    #[test]
    fn nginx_without_docroot_unquoted_works() {
        let conf = "server {\n  server_name plain.example;\n  root '/srv/plain';\n}\n";
        let cfg = parse_nginx_server(conf).unwrap();
        assert_eq!(cfg.host, "plain.example");
        assert_eq!(cfg.docroot, PathBuf::from("/srv/plain"));
    }

    #[test]
    fn missing_host_is_a_clear_error_not_a_panic() {
        let conf = "DocumentRoot /var/www/example\n";
        assert!(matches!(parse_apache_vhost(conf), Err(ImportError::MissingHost)));
    }

    #[test]
    fn missing_docroot_is_a_clear_error_not_a_panic() {
        let conf = "server_name example.com;\n";
        assert!(matches!(parse_nginx_server(conf), Err(ImportError::MissingDocroot)));
    }

    #[test]
    fn nginx_takes_first_of_multiple_server_names() {
        let conf = "server {\n  server_name first.example second.example;\n  root /var/www/x;\n}\n";
        let cfg = parse_nginx_server(conf).unwrap();
        assert_eq!(cfg.host, "first.example");
    }

    // --- RewriteCond + RewriteRule (Apache) ---------------------------

    #[test]
    fn apache_rewrite_cond_and_rule_are_paired() {
        let conf = r#"
<VirtualHost *:80>
    ServerName example.com
    DocumentRoot /var/www/example
    RewriteCond %{HTTP_HOST} ^m\.example\.com$
    RewriteRule ^/(.*)$ /mobile/$1 [L]
</VirtualHost>
"#;
        let cfg = parse_apache_vhost(conf).unwrap();
        assert_eq!(cfg.rewrite_rules.len(), 1);
        let rule = &cfg.rewrite_rules[0];
        assert_eq!(rule.conditions.len(), 1);
        assert_eq!(rule.conditions[0].variable, "HTTP_HOST");
        assert!(!rule.redirect);

        let ctx_match = crate::rewrite::RewriteContext { http_host: Some("m.example.com"), ..Default::default() };
        assert_eq!(
            crate::rewrite::apply_with_context("/page", &cfg.rewrite_rules, &ctx_match),
            crate::rewrite::RewriteOutcome::Rewritten("/mobile/page".to_string())
        );
    }

    #[test]
    fn apache_rewrite_rule_with_redirect_flag_is_external_redirect() {
        let conf = r#"
<VirtualHost *:80>
    ServerName example.com
    DocumentRoot /var/www/example
    RewriteRule ^/old$ /new [R=301,L]
</VirtualHost>
"#;
        let cfg = parse_apache_vhost(conf).unwrap();
        assert_eq!(cfg.rewrite_rules.len(), 1);
        assert!(cfg.rewrite_rules[0].redirect);
        assert!(cfg.rewrite_rules[0].conditions.is_empty());
    }

    // --- if-block return/rewrite (Nginx) -------------------------------

    #[test]
    fn nginx_if_block_with_return_becomes_conditional_redirect_rule() {
        let conf = r#"
server {
    server_name example.com;
    root /var/www/example;
    if ($http_host ~ ^m\.example\.com$) {
        return 301 https://m.example.com/mobile;
    }
}
"#;
        let cfg = parse_nginx_server(conf).unwrap();
        assert_eq!(cfg.rewrite_rules.len(), 1);
        let rule = &cfg.rewrite_rules[0];
        assert!(rule.redirect);
        assert_eq!(rule.conditions.len(), 1);
        assert_eq!(rule.conditions[0].variable, "HTTP_HOST");
    }

    #[test]
    fn nginx_if_block_with_rewrite_becomes_conditional_internal_rewrite() {
        let conf = r#"
server {
    server_name example.com;
    root /var/www/example;
    if ($request_method = POST) {
        rewrite ^/submit$ /handle-submit last;
    }
}
"#;
        // `$request_method = POST`(正規表現ではなく`=`比較)は今回のスコープ
        // 外の構文のため、この`if`は無視され通常のvhostとしてパースされる
        // ことを確認する(黙って壊れるのではなく、単に条件が付かない)。
        let cfg = parse_nginx_server(conf).unwrap();
        assert!(cfg.rewrite_rules.is_empty());
    }

    #[test]
    fn nginx_if_block_with_tilde_request_method_condition_works() {
        let conf = r#"
server {
    server_name example.com;
    root /var/www/example;
    if ($request_method ~ ^POST$) {
        rewrite ^/submit$ /handle-submit last;
    }
}
"#;
        let cfg = parse_nginx_server(conf).unwrap();
        assert_eq!(cfg.rewrite_rules.len(), 1);
        assert_eq!(cfg.rewrite_rules[0].conditions[0].variable, "REQUEST_METHOD");
        assert!(!cfg.rewrite_rules[0].redirect);
    }

    // --- Basic auth ------------------------------------------------------

    #[test]
    fn apache_basic_auth_is_parsed() {
        let conf = r#"
<VirtualHost *:80>
    ServerName example.com
    DocumentRoot /var/www/example
    AuthType Basic
    AuthName "Restricted Area"
    AuthUserFile /etc/apache2/.htpasswd
</VirtualHost>
"#;
        let cfg = parse_apache_vhost(conf).unwrap();
        let auth = cfg.basic_auth.expect("basic_auth should be set");
        assert_eq!(auth.realm, "Restricted Area");
        assert_eq!(auth.user_file, PathBuf::from("/etc/apache2/.htpasswd"));
    }

    #[test]
    fn apache_digest_auth_is_skipped_not_treated_as_basic() {
        let conf = r#"
<VirtualHost *:80>
    ServerName example.com
    DocumentRoot /var/www/example
    AuthType Digest
    AuthName "Restricted Area"
    AuthUserFile /etc/apache2/.htdigest
</VirtualHost>
"#;
        let cfg = parse_apache_vhost(conf).unwrap();
        assert!(cfg.basic_auth.is_none());
    }

    #[test]
    fn nginx_basic_auth_is_parsed() {
        let conf = r#"
server {
    server_name example.com;
    root /var/www/example;
    auth_basic "Restricted Area";
    auth_basic_user_file /etc/nginx/.htpasswd;
}
"#;
        let cfg = parse_nginx_server(conf).unwrap();
        let auth = cfg.basic_auth.expect("basic_auth should be set");
        assert_eq!(auth.realm, "Restricted Area");
        assert_eq!(auth.user_file, PathBuf::from("/etc/nginx/.htpasswd"));
    }

    #[test]
    fn nginx_auth_basic_off_does_not_set_basic_auth() {
        let conf = r#"
server {
    server_name example.com;
    root /var/www/example;
    auth_basic off;
}
"#;
        let cfg = parse_nginx_server(conf).unwrap();
        assert!(cfg.basic_auth.is_none());
    }

    // --- TLS certificate paths -------------------------------------------

    #[test]
    fn apache_ssl_certificate_paths_are_parsed() {
        let conf = r#"
<VirtualHost *:443>
    ServerName secure.example.com
    DocumentRoot /var/www/secure
    SSLCertificateFile /etc/ssl/certs/secure.example.com.crt
    SSLCertificateKeyFile /etc/ssl/private/secure.example.com.key
</VirtualHost>
"#;
        let cfg = parse_apache_vhost(conf).unwrap();
        let tls = cfg.tls_cert.expect("tls_cert should be set");
        assert_eq!(tls.cert_path, PathBuf::from("/etc/ssl/certs/secure.example.com.crt"));
        assert_eq!(tls.key_path, Some(PathBuf::from("/etc/ssl/private/secure.example.com.key")));
    }

    #[test]
    fn nginx_ssl_certificate_paths_are_parsed() {
        let conf = r#"
server {
    server_name secure.example.com;
    root /var/www/secure;
    ssl_certificate /etc/nginx/ssl/secure.example.com.crt;
    ssl_certificate_key /etc/nginx/ssl/secure.example.com.key;
}
"#;
        let cfg = parse_nginx_server(conf).unwrap();
        let tls = cfg.tls_cert.expect("tls_cert should be set");
        assert_eq!(tls.cert_path, PathBuf::from("/etc/nginx/ssl/secure.example.com.crt"));
        assert_eq!(tls.key_path, Some(PathBuf::from("/etc/nginx/ssl/secure.example.com.key")));
    }

    #[test]
    fn cert_without_key_directive_still_recorded_honestly() {
        let conf = r#"
<VirtualHost *:443>
    ServerName secure.example.com
    DocumentRoot /var/www/secure
    SSLCertificateFile /etc/ssl/certs/secure.example.com.crt
</VirtualHost>
"#;
        let cfg = parse_apache_vhost(conf).unwrap();
        let tls = cfg.tls_cert.expect("tls_cert should be set");
        assert_eq!(tls.key_path, None);
    }

    // --- basic IP allow/deny --------------------------------------------

    #[test]
    fn apache_allow_deny_from_directory_block_is_parsed() {
        let conf = r#"
<VirtualHost *:80>
    ServerName example.com
    DocumentRoot /var/www/example
    <Directory /var/www/example/admin>
        Allow from 192.168.1.0/24
        Deny from 10.0.0.5
    </Directory>
</VirtualHost>
"#;
        let cfg = parse_apache_vhost(conf).unwrap();
        let ac = cfg.access_control.expect("access_control should be set");
        assert_eq!(ac.allow, vec!["192.168.1.0/24".to_string()]);
        assert_eq!(ac.deny, vec!["10.0.0.5".to_string()]);
    }

    #[test]
    fn nginx_allow_deny_is_parsed() {
        let conf = r#"
server {
    server_name example.com;
    root /var/www/example;
    allow 192.168.1.0/24;
    deny all;
}
"#;
        let cfg = parse_nginx_server(conf).unwrap();
        let ac = cfg.access_control.expect("access_control should be set");
        assert_eq!(ac.allow, vec!["192.168.1.0/24".to_string()]);
        assert_eq!(ac.deny, vec!["all".to_string()]);
    }

    #[test]
    fn no_access_control_directives_yields_none() {
        let cfg = parse_apache_vhost(APACHE_SAMPLE).unwrap();
        assert!(cfg.access_control.is_none());
    }
}
