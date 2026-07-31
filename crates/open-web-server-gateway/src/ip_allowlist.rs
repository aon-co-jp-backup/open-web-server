//! 管理API向けIPアドレス許可リスト(2026-07-30新設)。
//!
//! **MACアドレスは採用しない、正直な理由**: MACアドレスはEthernet/Wi-Fi
//! フレームというローカルリンク層の情報であり、ルーターを1つ越えるたびに
//! 破棄・再生成される(IPパケットがルーティングされる際、MACアドレスは
//! 次ホップのものに書き換わる)。インターネット経由でサーバーに届く
//! リクエストから送信元の実MACアドレスを知る手段は無い——「MACアドレスで
//! 確認する」という要望は技術的に実現不可能なため、代わりに実際に検証
//! 可能な送信元IPアドレスでの許可リストを実装する。
//!
//! `OPEN_WEB_SERVER_ADMIN_ALLOWED_IPS`環境変数(カンマ区切り、単一IP
//! または`192.168.1.0/24`のようなCIDR表記)が設定されている場合のみ
//! 有効(既定は無効=既存動作を変えない、オプトイン)。設定されている
//! 場合、リストに一致しない送信元からの管理API(`/admin/*`・
//! `/internal/*`)アクセスは、`x-admin-token`/APIキーの正誤に関わらず
//! 拒否する(多層防御——正しいトークンを盗まれても、許可IP以外からは
//! 到達できない)。

use std::net::IpAddr;

/// 単一IPまたはCIDR表記の1エントリ。
enum AllowEntry {
    Exact(IpAddr),
    Cidr { network: IpAddr, prefix_len: u8 },
}

impl AllowEntry {
    fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if let Some((addr_part, prefix_part)) = s.split_once('/') {
            let network: IpAddr = addr_part.parse().ok()?;
            let prefix_len: u8 = prefix_part.parse().ok()?;
            let max_prefix = match network {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            if prefix_len > max_prefix {
                return None;
            }
            Some(AllowEntry::Cidr { network, prefix_len })
        } else {
            let addr: IpAddr = s.parse().ok()?;
            Some(AllowEntry::Exact(addr))
        }
    }

    fn matches(&self, candidate: IpAddr) -> bool {
        match self {
            AllowEntry::Exact(addr) => *addr == candidate,
            AllowEntry::Cidr { network, prefix_len } => match (network, candidate) {
                (IpAddr::V4(net), IpAddr::V4(cand)) => {
                    let mask = if *prefix_len == 0 { 0u32 } else { u32::MAX << (32 - prefix_len) };
                    (u32::from_be_bytes(net.octets()) & mask) == (u32::from_be_bytes(cand.octets()) & mask)
                }
                (IpAddr::V6(net), IpAddr::V6(cand)) => {
                    let mask = if *prefix_len == 0 { 0u128 } else { u128::MAX << (128 - prefix_len) };
                    (u128::from_be_bytes(net.octets()) & mask) == (u128::from_be_bytes(cand.octets()) & mask)
                }
                _ => false,
            },
        }
    }
}

/// `OPEN_WEB_SERVER_ADMIN_ALLOWED_IPS`から許可リストを読み込む。
/// 未設定・空文字列なら`None`(=許可リスト機能そのものが無効)。
pub fn allowlist_from_env() -> Option<Vec<String>> {
    let raw = std::env::var("OPEN_WEB_SERVER_ADMIN_ALLOWED_IPS").ok()?;
    let entries: Vec<String> = raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// `candidate`が許可リストのいずれかのエントリに一致するか。
/// 許可リスト自体が空(=`allowlist_from_env`が`None`)の場合、
/// この関数は呼ばれない設計(呼び出し側で先に`None`分岐を処理する)。
pub fn is_allowed(entries: &[String], candidate: IpAddr) -> bool {
    entries.iter().filter_map(|s| AllowEntry::parse(s)).any(|entry| entry.matches(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_ipv4_match() {
        let entries = vec!["203.0.113.5".to_string()];
        assert!(is_allowed(&entries, "203.0.113.5".parse().unwrap()));
        assert!(!is_allowed(&entries, "203.0.113.6".parse().unwrap()));
    }

    #[test]
    fn cidr_ipv4_match() {
        let entries = vec!["192.168.1.0/24".to_string()];
        assert!(is_allowed(&entries, "192.168.1.42".parse().unwrap()));
        assert!(!is_allowed(&entries, "192.168.2.1".parse().unwrap()));
    }

    #[test]
    fn cidr_ipv6_match() {
        let entries = vec!["2001:db8::/32".to_string()];
        assert!(is_allowed(&entries, "2001:db8::1".parse().unwrap()));
        assert!(!is_allowed(&entries, "2001:db9::1".parse().unwrap()));
    }

    #[test]
    fn malformed_entries_are_skipped_not_panicking() {
        let entries = vec!["not-an-ip".to_string(), "203.0.113.5".to_string()];
        assert!(is_allowed(&entries, "203.0.113.5".parse().unwrap()));
    }

    #[test]
    fn multiple_entries_any_match_allows() {
        let entries = vec!["10.0.0.1".to_string(), "192.168.1.0/24".to_string()];
        assert!(is_allowed(&entries, "192.168.1.5".parse().unwrap()));
        assert!(!is_allowed(&entries, "8.8.8.8".parse().unwrap()));
    }
}
