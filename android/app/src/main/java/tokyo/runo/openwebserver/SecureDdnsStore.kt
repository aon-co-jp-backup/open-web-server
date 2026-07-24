package tokyo.runo.openwebserver

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey

/**
 * DuckDNSトークン・管理APIトークン(`x-admin-token`)の安全な永続化
 * (2026-07-24新設、DDNSドメイン設定UI追加に伴う)。
 *
 * **セキュリティ方針(ユーザー指示)**: これらのトークンを平文の
 * `SharedPreferences`へ保存しない。Android推奨の
 * `EncryptedSharedPreferences`(`androidx.security.crypto`、Android
 * Keystoreで保護されたマスターキーによるAES-256-GCM/AES-256-SIV暗号化)を
 * 使う。ログへの出力も一切行わない(呼び出し元でも本ファイルの値を
 * `Log.*`へ渡さないこと)。
 *
 * 保存する2つの値:
 * - `adminToken` — このAndroid端末上で起動する`open-web-server`プロセスの
 *   `OPEN_WEB_SERVER_ADMIN_TOKEN`と一致させる管理APIトークン。
 *   `MainActivity`がサーバープロセス起動時にこの値を環境変数として渡し、
 *   `DdnsSetupActivity`が管理API呼び出し時に`x-admin-token`ヘッダとして送る
 *   ——両者が同じ値を共有することで、動的に生成/入力したトークンで
 *   管理APIを保護できる。
 * - `lastDuckDnsToken` — 直近入力したDuckDNSトークン(次回起動時の入力欄
 *   プリフィル用、UX目的のみ。サーバー側へは`setup-free-domain`呼び出し時に
 *   一度だけ送信され、恒久化はサーバー側の環境変数設定に委ねる——既存の
 *   `free_domain.rs`のスコープ通り)。
 */
object SecureDdnsStore {
    private const val PREFS_NAME = "open_web_server_secure_ddns_prefs"
    private const val KEY_ADMIN_TOKEN = "admin_token"
    private const val KEY_LAST_DUCKDNS_TOKEN = "last_duckdns_token"

    private fun prefs(context: Context): SharedPreferences {
        val masterKey = MasterKey.Builder(context.applicationContext)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        return EncryptedSharedPreferences.create(
            context.applicationContext,
            PREFS_NAME,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )
    }

    fun getAdminToken(context: Context): String? =
        prefs(context).getString(KEY_ADMIN_TOKEN, null)?.takeIf { it.isNotBlank() }

    fun setAdminToken(context: Context, token: String) {
        prefs(context).edit().putString(KEY_ADMIN_TOKEN, token).apply()
    }

    fun getLastDuckDnsToken(context: Context): String? =
        prefs(context).getString(KEY_LAST_DUCKDNS_TOKEN, null)

    fun setLastDuckDnsToken(context: Context, token: String) {
        prefs(context).edit().putString(KEY_LAST_DUCKDNS_TOKEN, token).apply()
    }
}
