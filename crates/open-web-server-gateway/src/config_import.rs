//! 実際のApache/Nginx設定ファイルから`WebVhostConfig`の基本部分
//! (ホスト名・ドキュメントルート・PHP-FPM接続先)を読み取るインポート
//! 機能(2026-08-03新設、改善計画「(3) 実設定ファイルのパース/インポート」
//! 対応、ユーザー指示によりスコープを「vhost定義の基本部分」に限定)。
//!
//! **正直な開示・スコープ**: Apache `httpd.conf`/Nginxの完全な設定言語
//! ("include"、変数展開、`<Location>`/`<Directory>`アクセス制御、
//! SSL証明書指定等)を解釈するフルパーサーではない。対応するのは
//! 以下の最小限のディレクティブのみ:
//! - **Apache**(`<VirtualHost>`ブロック内): `ServerName`
//!   (最初の1つを採用、`ServerAlias`は無視)、`DocumentRoot`
//!   (引用符の有無いずれも許容)、`SetHandler "proxy:fcgi://host:port"`
//!   があれば`PhpMode::FastCgi`として検出。
//! - **Nginx**(`server { }`ブロック内): `server_name`
//!   (複数指定時は最初の1つを採用)、`root`、`fastcgi_pass host:port;`
//!   があれば`PhpMode::FastCgi`として検出。
//! 上記に該当しない行(`Rewrite*`・`location`ブロックの複雑な条件分岐・
//! SSL関連ディレクティブ等)は単純に無視し、エラーにはしない——
//! 「読めるものは読み、読めないものは無視する」という寛容な設計
//! (設定ファイル全体を完全に理解できないと1件もインポートできない、
//! という体験を避けるため)。

use std::path::PathBuf;

use crate::web_vhost::{PhpMode, WebVhostConfig};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("no ServerName/server_name directive found in the provided config")]
    MissingHost,
    #[error("no DocumentRoot/root directive found in the provided config")]
    MissingDocroot,
}

/// Apacheの`<VirtualHost>...</VirtualHost>`ブロック(そのブロックの
/// 中身のみ、または`<VirtualHost>`行自体を含むテキストのどちらでも可)
/// から`WebVhostConfig`の基本部分を読み取る。
pub fn parse_apache_vhost(conf: &str) -> Result<WebVhostConfig, ImportError> {
    let mut host: Option<String> = None;
    let mut docroot: Option<String> = None;
    let mut fastcgi_addr: Option<String> = None;

    for raw_line in conf.lines() {
        let line = raw_line.trim();
        if let Some(rest) = strip_directive(line, "ServerName") {
            host.get_or_insert_with(|| rest.to_string());
        } else if let Some(rest) = strip_directive(line, "DocumentRoot") {
            docroot.get_or_insert_with(|| unquote(rest).to_string());
        } else if let Some(rest) = strip_directive(line, "SetHandler") {
            // 例: SetHandler "proxy:fcgi://127.0.0.1:9000"
            let value = unquote(rest);
            if let Some(addr) = value.strip_prefix("proxy:fcgi://") {
                fastcgi_addr = Some(addr.to_string());
            }
        }
    }

    let host = host.ok_or(ImportError::MissingHost)?;
    let docroot = docroot.ok_or(ImportError::MissingDocroot)?;
    Ok(build_config(host, docroot, fastcgi_addr))
}

/// Nginxの`server { ... }`ブロック(ブロックの中身のみ、または
/// `server {`行自体を含むテキストのどちらでも可)から`WebVhostConfig`の
/// 基本部分を読み取る。
pub fn parse_nginx_server(conf: &str) -> Result<WebVhostConfig, ImportError> {
    let mut host: Option<String> = None;
    let mut docroot: Option<String> = None;
    let mut fastcgi_addr: Option<String> = None;

    for raw_line in conf.lines() {
        let line = raw_line.trim().trim_end_matches(';').trim();
        if let Some(rest) = strip_directive(line, "server_name") {
            // 複数ホスト指定(スペース区切り)のうち最初の1つを採用。
            host.get_or_insert_with(|| rest.split_whitespace().next().unwrap_or(rest).to_string());
        } else if let Some(rest) = strip_directive(line, "root") {
            docroot.get_or_insert_with(|| unquote(rest).to_string());
        } else if let Some(rest) = strip_directive(line, "fastcgi_pass") {
            fastcgi_addr.get_or_insert_with(|| unquote(rest).to_string());
        }
    }

    let host = host.ok_or(ImportError::MissingHost)?;
    let docroot = docroot.ok_or(ImportError::MissingDocroot)?;
    Ok(build_config(host, docroot, fastcgi_addr))
}

fn build_config(host: String, docroot: String, fastcgi_addr: Option<String>) -> WebVhostConfig {
    let (php_enabled, php_mode) = match fastcgi_addr {
        Some(addr) => (true, PhpMode::FastCgi { fastcgi_addr: addr }),
        None => (false, PhpMode::default()),
    };
    WebVhostConfig {
        host,
        docroot: PathBuf::from(docroot),
        php_enabled,
        compat_mode: Default::default(),
        php_mode,
        rewrite_rules: Vec::new(),
    }
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
}
