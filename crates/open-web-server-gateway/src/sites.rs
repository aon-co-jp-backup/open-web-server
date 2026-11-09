//! `sites.toml` — 1サイト1宣言の設定(分身の術ホスティング、段階1)。
//!
//! 仕様: `open-easy-web/docs/RPOEM_BUNSHIN_HOSTING_SPEC.md`。
//! 既存の `domains.toml` / `web_vhosts.toml` は一切変えず、この設定を
//! 既存の `TenantConfig`(kind=process)と `WebVhostConfig`(static/fastcgi)
//! へ展開するだけの薄い層。`OPEN_WEB_SERVER_SITES_FILE`で有効になる。
//!
//! 段階1の範囲: 展開・同時接続上限(processのみ)・ポート自動割当。
//! 別ユーザー起動/cgroup/子プロセス監督は段階2以降(未実装)。

use std::collections::HashSet;
use std::path::PathBuf;

use serde::Deserialize;

use crate::tenant_router::{BackendKind, TenantConfig};
use crate::web_vhost::{PhpMode, WebVhostConfig};

/// ポート自動割当の範囲(`[runtime] port_range`で上書き可)。
const DEFAULT_PORT_RANGE: (u16, u16) = (20000, 29999);

#[derive(Debug, Deserialize, Default)]
pub struct SitesFile {
    #[serde(default)]
    pub runtime: RuntimeSection,
    #[serde(rename = "site", default)]
    pub sites: Vec<SiteConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RuntimeSection {
    /// 将来用(段階2以降)。段階1は読むだけで使わない。
    #[serde(default)]
    pub worker_threads: u32,
    #[serde(default)]
    pub port_range: Option<(u16, u16)>,
    /// `trusted`(既定)のみ有効。`tenant`は未実装なので拒否する。
    #[serde(default)]
    pub isolation: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SiteKind {
    Static,
    Fastcgi,
    Process,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Limits {
    pub max_concurrent: Option<u32>,
}

/// `port = 9001` か `port = "auto"` を受ける。
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum PortSpec {
    Fixed(u16),
    Auto(String),
}

#[derive(Debug, Deserialize, Clone)]
pub struct PhpSection {
    /// `127.0.0.1:9000` またはUnixソケットパス。
    pub fastcgi_addr: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SiteConfig {
    pub id: String,
    pub hosts: Vec<String>,
    pub kind: SiteKind,
    #[serde(default)]
    pub path_prefix: Option<String>,
    #[serde(default)]
    pub root: Option<PathBuf>,
    #[serde(default)]
    pub php: Option<PhpSection>,
    #[serde(default)]
    pub port: Option<PortSpec>,
    #[serde(default)]
    pub db_uri: Option<String>,
    #[serde(default)]
    pub limits: Limits,
    /// 既定true(`TenantConfig::strip_prefix`と同じ)。
    #[serde(default)]
    pub strip_prefix: Option<bool>,
}

/// 展開結果。
#[derive(Debug, Default)]
pub struct Expanded {
    pub tenants: Vec<TenantConfig>,
    pub vhosts: Vec<WebVhostConfig>,
    /// 自動割当したポート(site id, port)。ログ用。
    pub assigned_ports: Vec<(String, u16)>,
}

/// 自動割当ポートが使えるかの判定(実際にbindできるか)。
pub fn port_is_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn stable_hash(s: &str) -> u64 {
    // FNV-1a(再起動しても同じ値になる。std HashMapのランダムシードは使わない)。
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// idのハッシュ位置から範囲内を線形に探し、他サイトに割当済みでなく、
/// かつ空いているポートを返す。同じ構成なら再起動後も同じポートになる。
fn auto_port(
    id: &str,
    range: (u16, u16),
    taken: &HashSet<u16>,
    is_free: &dyn Fn(u16) -> bool,
) -> Option<u16> {
    let (lo, hi) = range;
    if lo > hi {
        return None;
    }
    let span = (hi - lo) as u64 + 1;
    let start = stable_hash(id) % span;
    for i in 0..span {
        let p = lo + ((start + i) % span) as u16;
        if !taken.contains(&p) && is_free(p) {
            return Some(p);
        }
    }
    None
}

pub fn parse_and_expand(
    toml_str: &str,
    is_free: &dyn Fn(u16) -> bool,
) -> Result<Expanded, String> {
    let file: SitesFile =
        toml::from_str(toml_str).map_err(|e| format!("sites.toml解析失敗: {e}"))?;
    match file.runtime.isolation.as_deref() {
        None | Some("trusted") => {}
        Some(other) => {
            return Err(format!(
                "isolation = \"{other}\" は未実装です(現在は trusted のみ)"
            ))
        }
    }
    let range = file.runtime.port_range.unwrap_or(DEFAULT_PORT_RANGE);

    let mut ids = HashSet::new();
    let mut routes = HashSet::new();
    let mut taken: HashSet<u16> = HashSet::new();
    // 固定ポートを先に確保して、自動割当と衝突させない。
    for s in &file.sites {
        if let Some(PortSpec::Fixed(p)) = &s.port {
            if !taken.insert(*p) {
                return Err(format!("site '{}': ポート {p} が他のサイトと重複", s.id));
            }
        }
    }

    let mut out = Expanded::default();
    for s in &file.sites {
        if s.id.is_empty()
            || !s
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!("site id '{}' は英数字・-・_ のみ", s.id));
        }
        if !ids.insert(s.id.clone()) {
            return Err(format!("site id '{}' が重複", s.id));
        }
        if s.hosts.is_empty() {
            return Err(format!("site '{}': hosts が空", s.id));
        }
        for h in &s.hosts {
            let key = (h.to_ascii_lowercase(), s.path_prefix.clone());
            if !routes.insert(key) {
                return Err(format!("site '{}': host '{h}' + path_prefix が重複", s.id));
            }
        }
        match s.kind {
            SiteKind::Process => {
                let port = match &s.port {
                    Some(PortSpec::Fixed(p)) => *p,
                    Some(PortSpec::Auto(a)) if a == "auto" => {
                        let p = auto_port(&s.id, range, &taken, is_free).ok_or_else(|| {
                            format!("site '{}': 範囲 {range:?} に空きポートがない", s.id)
                        })?;
                        taken.insert(p);
                        out.assigned_ports.push((s.id.clone(), p));
                        p
                    }
                    Some(PortSpec::Auto(a)) => {
                        return Err(format!(
                            "site '{}': port '{a}' は不正(数値か \"auto\")",
                            s.id
                        ))
                    }
                    None => return Err(format!("site '{}': process には port が必要", s.id)),
                };
                for h in &s.hosts {
                    out.tenants.push(TenantConfig {
                        host: h.to_ascii_lowercase(),
                        backend: BackendKind::OpenRuno,
                        backend_addr: format!("127.0.0.1:{port}"),
                        db_uri: s.db_uri.clone().unwrap_or_default(),
                        path_prefix: s.path_prefix.clone(),
                        override_host: None,
                        strip_prefix: s.strip_prefix.unwrap_or(true),
                        max_concurrent: s.limits.max_concurrent,
                        managed_by: Some(s.id.clone()),
                    });
                }
            }
            SiteKind::Static | SiteKind::Fastcgi => {
                if s.path_prefix.as_deref().is_some_and(|p| p != "/") {
                    return Err(format!(
                        "site '{}': static/fastcgi の path_prefix は段階1では未対応",
                        s.id
                    ));
                }
                let root = s
                    .root
                    .clone()
                    .ok_or_else(|| format!("site '{}': root が必要", s.id))?;
                let php_mode = match (s.kind, &s.php) {
                    (SiteKind::Fastcgi, Some(p)) => PhpMode::FastCgi {
                        fastcgi_addr: p.fastcgi_addr.clone(),
                    },
                    (SiteKind::Fastcgi, None) => {
                        return Err(format!(
                            "site '{}': fastcgi には php.fastcgi_addr が必要",
                            s.id
                        ))
                    }
                    _ => PhpMode::default(),
                };
                for h in &s.hosts {
                    let src = format!(
                        "host = {:?}\ndocroot = {:?}\nphp_enabled = {}\n",
                        h.to_ascii_lowercase(),
                        root.to_string_lossy(),
                        s.kind == SiteKind::Fastcgi
                    );
                    let mut v: WebVhostConfig = toml::from_str(&src)
                        .map_err(|e| format!("site '{}': vhost生成失敗: {e}", s.id))?;
                    v.php_mode = php_mode.clone();
                    out.vhosts.push(v);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_FREE: &dyn Fn(u16) -> bool = &|_| true;

    #[test]
    fn expands_process_static_and_fastcgi() {
        let t = r#"
[[site]]
id = "app1"
hosts = ["App1.example.com"]
kind = "process"
port = "auto"
limits = { max_concurrent = 4 }

[[site]]
id = "blog"
hosts = ["blog.example.com"]
kind = "static"
root = "/srv/sites/blog/public"

[[site]]
id = "shop"
hosts = ["shop.example.com"]
kind = "fastcgi"
root = "/srv/sites/shop/public"
php = { fastcgi_addr = "127.0.0.1:9000" }
"#;
        let e = parse_and_expand(t, ALL_FREE).unwrap();
        assert_eq!(e.tenants.len(), 1);
        assert_eq!(e.tenants[0].host, "app1.example.com");
        assert_eq!(e.tenants[0].max_concurrent, Some(4));
        assert_eq!(e.tenants[0].managed_by.as_deref(), Some("app1"));
        assert_eq!(e.vhosts.len(), 2);
        assert!(!e.vhosts[0].php_enabled);
        assert!(e.vhosts[1].php_enabled);
        assert!(matches!(e.vhosts[1].php_mode, PhpMode::FastCgi { .. }));
        let p = e.assigned_ports[0].1;
        assert!((20000..=29999).contains(&p));
    }

    #[test]
    fn auto_port_is_stable_unique_and_skips_busy_and_fixed() {
        let t = r#"
[[site]]
id = "a"
hosts = ["a.example.com"]
kind = "process"
port = "auto"
[[site]]
id = "b"
hosts = ["b.example.com"]
kind = "process"
port = "auto"
[[site]]
id = "c"
hosts = ["c.example.com"]
kind = "process"
port = 20001
"#;
        let t1 = parse_and_expand(t, ALL_FREE).unwrap();
        let t2 = parse_and_expand(t, ALL_FREE).unwrap();
        assert_eq!(t1.assigned_ports, t2.assigned_ports, "再起動しても同じ");
        let ports: HashSet<u16> = t1.assigned_ports.iter().map(|x| x.1).collect();
        assert_eq!(ports.len(), 2);
        assert!(!ports.contains(&20001), "固定ポートと衝突しない");
        let busy = t1.assigned_ports[0].1;
        let t3 = parse_and_expand(t, &move |p| p != busy).unwrap();
        assert_ne!(t3.assigned_ports[0].1, busy);
    }

    #[test]
    fn rejects_bad_input() {
        let dup = "[[site]]\nid='a'\nhosts=['x.com']\nkind='static'\nroot='/r'\n[[site]]\nid='a'\nhosts=['y.com']\nkind='static'\nroot='/r'\n";
        assert!(parse_and_expand(dup, ALL_FREE).unwrap_err().contains("重複"));
        let dup_host = "[[site]]\nid='a'\nhosts=['x.com']\nkind='static'\nroot='/r'\n[[site]]\nid='b'\nhosts=['X.com']\nkind='static'\nroot='/r'\n";
        assert!(parse_and_expand(dup_host, ALL_FREE).is_err());
        let tenant = "[runtime]\nisolation='tenant'\n";
        assert!(parse_and_expand(tenant, ALL_FREE)
            .unwrap_err()
            .contains("未実装"));
        let noport = "[[site]]\nid='a'\nhosts=['x.com']\nkind='process'\n";
        assert!(parse_and_expand(noport, ALL_FREE).is_err());
        let badid = "[[site]]\nid='a b'\nhosts=['x.com']\nkind='static'\nroot='/r'\n";
        assert!(parse_and_expand(badid, ALL_FREE).is_err());
    }
}
