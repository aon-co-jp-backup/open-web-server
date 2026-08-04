package tokyo.runo.openwebserver

import android.content.Context
import android.content.SharedPreferences

/**
 * root化した端末で、USB OTG接続の外付けHDDをサーバーの主ストレージ
 * (`web_vhosts.toml`・ドメイン登録・TLS証明書・KeyGuardianキーストア等の
 * 実データ保存先)として使うための設定(2026-08-04新設、ユーザー指示
 * 「root化してでもHDDを主ストレージにしたい」への対応)。
 *
 * **正直な前提(誇張しない)**: Android 10+のScoped Storage制限により、
 * root化していない端末では外部USBストレージへネイティブバイナリが
 * 直接POSIXファイルパスで読み書きすることはできない(SAF経由の
 * `content://`URIしか得られず、`std::fs`は使えない)。この機能は
 * **root化済みの端末専用**であり、非root端末では`MainActivity`側が
 * `su`の到達性チェックで明確に検出し、有効化されていても起動を拒否して
 * 理由を表示する(黙って内部ストレージへフォールバックしない——
 * ユーザーが「HDDに保存されているはず」と誤認したまま運用する事故を
 * 避けるため)。
 *
 * 保存する値自体(マウントパス・有効フラグ)は秘密情報ではないため、
 * `SecureDdnsStore`(EncryptedSharedPreferences)とは異なり素の
 * `SharedPreferences`で十分と判断した。
 */
object ExternalStorageConfig {
    private const val PREFS_NAME = "open_web_server_external_storage_prefs"
    private const val KEY_ENABLED = "enabled"
    private const val KEY_MOUNT_PATH = "mount_path"

    /** サーバーの実データを置くサブディレクトリ名(マウントパス配下)。 */
    const val DATA_SUBDIR = "open-web-server-data"

    private fun prefs(context: Context): SharedPreferences =
        context.applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    fun isEnabled(context: Context): Boolean = prefs(context).getBoolean(KEY_ENABLED, false)

    fun getMountPath(context: Context): String? =
        prefs(context).getString(KEY_MOUNT_PATH, null)?.takeIf { it.isNotBlank() }

    fun save(context: Context, enabled: Boolean, mountPath: String) {
        prefs(context).edit()
            .putBoolean(KEY_ENABLED, enabled)
            .putString(KEY_MOUNT_PATH, mountPath.trim())
            .apply()
    }

    /** `mountPath`配下の実データディレクトリの絶対パス(例: "/mnt/media_rw/XXXX/open-web-server-data")。 */
    fun dataDirPath(mountPath: String): String {
        val trimmed = mountPath.trim().trimEnd('/')
        return "$trimmed/$DATA_SUBDIR"
    }
}
