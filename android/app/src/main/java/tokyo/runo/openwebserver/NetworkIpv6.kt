package tokyo.runo.openwebserver

import android.content.Context
import android.net.ConnectivityManager
import java.net.Inet6Address

/**
 * 回線非依存(WiFi限定ではない)のグローバルIPv6アドレス取得
 * (2026-08-06新設)。
 *
 * **背景**: サーバー側(`crates/open-web-server-gateway/src/
 * custom_dns_ipv6_autoupdate.rs`)は、v6プラス(MAP-E)環境下の自宅
 * サーバーのように、IPv4はポート開放できないがIPv6は無制限に使える
 * 構成向けに、外部echoサービス(`https://api6.ipify.org`)経由でグローバル
 * IPv6を取得しAAAAレコードを自動更新する。Android版でも同じ状況
 * (固定IPを持たないスマホ/タブレットをv6プラス的な回線でサーバー化する
 * ユースケース)に対応するため、**このAndroid端末自身が今どのグローバル
 * IPv6アドレスを持っているか**をアプリ内で確認できるようにする。
 *
 * **「WiFi限定」だった問題点(修正対象)**: [MainActivity]の
 * `BindAddressPolicy.currentWifiIpv4()`は`android.net.wifi.WifiManager`
 * (WiFi接続時のみ有効なAPI)からIPv4アドレスを取得する設計であり、
 * モバイルデータ回線接続時には常に`null`を返す——IPv6についてもこの
 * 前例に倣ってWifiManager経由で実装すると同じ制約を引き継いでしまう。
 * そのため、本ユーティリティは`ConnectivityManager.getLinkProperties()`
 * (`activeNetwork`から取得、WiFi/モバイルデータ/イーサネット等の接続
 * 種別を問わない)を使い、**現在アクティブなネットワークが何であっても**
 * グローバルIPv6アドレスを検出できるようにした。
 *
 * **正直な開示・現状のスコープ**: (1) ここで検出したIPv6アドレスは
 * 表示・確認用途のみで、ネイティブサーバープロセス(`OPEN_WEB_SERVER_BIND`)
 * の実際のbindアドレスへは今回配線していない——`open-web-server`本体は
 * 単一の`host:port`にbindする設計のため、IPv4(既存の`BindAddressPolicy`)
 * とIPv6の両方へ同時にbindするデュアルスタック対応は本体(Rust)側の
 * 拡張が必要であり、今回のスコープ外として明記する。(2) キャリアが
 * 実際にグローバルルーティング可能なIPv6を割り当てているか
 * (CGNAT配下のIPv4のようにキャリア内部だけで有効なIPv6を割る場合が
 * 稀にあるかもしれない点)は、このAndroid端末側からは「リンクローカル
 * ([fe80::]で始まるアドレス)ではない」という判定でしか区別できない
 * ——真にインターネットから到達可能かどうかの確認は、既存のサーバー側
 * echoサービス方式(`api6.ipify.org`)や実際の外部からの接続テストに
 * 委ねる。
 */
object NetworkIpv6 {

    /**
     * 現在アクティブなネットワーク(WiFi/モバイルデータ/イーサネット等、
     * 接続種別を問わない)が持つグローバル(リンクローカルではない)
     * IPv6アドレスを返す。取得できない場合は`null`(例外を投げない)。
     */
    fun currentGlobalIpv6(context: Context): String? {
        return try {
            val cm = context.applicationContext
                .getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
                ?: return null
            val network = cm.activeNetwork ?: return null
            val linkProperties = cm.getLinkProperties(network) ?: return null
            linkProperties.linkAddresses
                .asSequence()
                .map { it.address }
                .filterIsInstance<Inet6Address>()
                .filterNot { it.isLinkLocalAddress }
                .filterNot { it.isLoopbackAddress }
                .filterNot { it.isMulticastAddress }
                .firstOrNull()
                ?.hostAddress
        } catch (_: Exception) {
            null
        }
    }

    /**
     * 現在の接続種別(表示用、日英併記)——検出結果がWiFi限定でないことを
     * ユーザーにも明示するための補助情報。
     */
    fun currentTransportSummary(context: Context): String {
        return try {
            val cm = context.applicationContext
                .getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
                ?: return "不明 / unknown"
            val network = cm.activeNetwork ?: return "接続なし / no active network"
            val capabilities = cm.getNetworkCapabilities(network) ?: return "不明 / unknown"
            when {
                capabilities.hasTransport(android.net.NetworkCapabilities.TRANSPORT_WIFI) ->
                    "WiFi"
                capabilities.hasTransport(android.net.NetworkCapabilities.TRANSPORT_CELLULAR) ->
                    "モバイルデータ / Cellular"
                capabilities.hasTransport(android.net.NetworkCapabilities.TRANSPORT_ETHERNET) ->
                    "イーサネット / Ethernet"
                else -> "その他 / other"
            }
        } catch (_: Exception) {
            "不明 / unknown"
        }
    }
}
