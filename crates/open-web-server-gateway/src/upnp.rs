//! UPnP IGD(Internet Gateway Device)による自動ポート開放の補助機能
//! (`upnp` feature、既定オフ)。
//!
//! NATの内側にいる場合、多くの家庭用ルーターはUPnP IGDプロトコルに対応して
//! おり、ソフトウェア側から自動でポートフォワーディングを要求できる。
//! `igd-next`(`igd`の後継、activeにメンテナンスされていることを2026-07
//! 時点でcrates.io/GitHub上で確認済み)を使う。
//!
//! # 正直な開示
//! UPnP非対応ルーター(企業ネットワーク・一部のISPレンタルルーター等)では
//! 失敗する。失敗時はパニックせず`tracing::warn!`で「手動でポート
//! フォワード設定が必要」という案内を出し、SFTPサーバー自体の起動は
//! 妨げない(補助機能が権威パスをブロックしないという既存方針
//! `ddns.rs`/`acme.rs`と同じ設計)。またこのクレートは実ルーターの無い
//! 開発環境では実機検証ができない場合がある(サンドボックス環境の制約、
//! 正直な開示パターンに従う)。

use std::net::SocketAddrV4;
use std::time::Duration;

const LEASE_DURATION_SECS: u32 = 0; // 0 = 恒久的(ルーターが対応していれば)。

/// `OPEN_WEB_SERVER_UPNP_AUTO_FORWARD=true` が設定されている場合のみ、
/// 与えられたポート群についてUPnP IGD経由の自動ポート開放をベストエフォート
/// で試行する。ユーザーのネットワーク機器を無断で操作しないため明示opt-in
/// 必須。
pub fn spawn_if_configured(ports: Vec<(u16, &'static str)>) {
    let enabled = std::env::var("OPEN_WEB_SERVER_UPNP_AUTO_FORWARD")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !enabled || ports.is_empty() {
        return;
    }

    tokio::spawn(async move {
        for (port, description) in ports {
            match forward_port(port, description).await {
                Ok(external_ip) => {
                    tracing::info!(port, external_ip = %external_ip, "UPnP: port forwarded successfully");
                }
                Err(e) => {
                    tracing::warn!(
                        port,
                        error = %e,
                        "UPnP: automatic port forwarding failed (router may not support UPnP IGD, \
                         or it is disabled/on a corporate network). Manual port forwarding configuration \
                         on your router is required for external access; this does not block the server \
                         itself from starting."
                    );
                }
            }
        }
    });
}

/// 単一ポートに対する`add_port`の実行。ルーター(gateway)発見自体が
/// タイムアウトし得るため、明示的なタイムアウトを掛ける。
async fn forward_port(port: u16, description: &str) -> anyhow::Result<std::net::Ipv4Addr> {
    let gateway = tokio::time::timeout(Duration::from_secs(5), igd_next::aio::tokio::search_gateway(Default::default()))
        .await
        .map_err(|_| anyhow::anyhow!("UPnP gateway discovery timed out"))??;

    let local_addr = local_ipv4_addr()?;
    let local_socket = SocketAddrV4::new(local_addr, port);

    gateway
        .add_port(
            igd_next::PortMappingProtocol::TCP,
            port,
            std::net::SocketAddr::V4(local_socket),
            LEASE_DURATION_SECS,
            description,
        )
        .await
        .map_err(|e| anyhow::anyhow!("add_port failed: {e}"))?;

    let external_ip = gateway.get_external_ip().await.map_err(|e| anyhow::anyhow!("failed to query external IP: {e}"))?;
    match external_ip {
        std::net::IpAddr::V4(v4) => Ok(v4),
        std::net::IpAddr::V6(_) => anyhow::bail!("gateway reported an IPv6 external address, expected IPv4"),
    }
}

/// UDPソケットをダミー接続させて、外向けに使われるローカルIPv4アドレスを
/// 推測する(既定ルート判定の定番トリック、実際にパケットは送信されない)。
fn local_ipv4_addr() -> anyhow::Result<std::net::Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("8.8.8.8:80")?;
    match socket.local_addr()?.ip() {
        std::net::IpAddr::V4(v4) => Ok(v4),
        std::net::IpAddr::V6(_) => anyhow::bail!("local address resolved to IPv6, UPnP IGDv1 requires IPv4"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_if_configured_is_a_noop_without_env_var() {
        std::env::remove_var("OPEN_WEB_SERVER_UPNP_AUTO_FORWARD");
        // パニックしない・何も起動しないことだけを確認(実ルーターが無い
        // 開発環境では実際のUPnP応答を検証できないため、既存の
        // `ddns::spawn_if_configured_is_a_noop_without_env_var`と同じ
        // 検証方針を踏襲する)。
        spawn_if_configured(vec![(2222, "test")]);
    }

    #[test]
    fn local_ipv4_addr_resolves_without_panicking() {
        // 実ネットワーク到達性の無いサンドボックス環境でも、UDPソケットの
        // bind/connectはローカルルーティングテーブル解決のみで完結するため
        // 失敗しにくいが、念のためエラーでもパニックしないことだけ確認する。
        let _ = local_ipv4_addr();
    }
}
