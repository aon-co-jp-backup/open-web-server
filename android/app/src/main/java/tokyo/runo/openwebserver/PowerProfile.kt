package tokyo.runo.openwebserver

import android.content.Context

/**
 * 3電源プロファイル(2026-07-24、ユーザー指示で追加)。
 *
 * - [POWER_SAVE] 省電力版: バックグラウンドでの常時稼働を避け、Android
 *   Doze/App Standbyに逆らわない(=`WakeLock`を一切取得しない)。
 * - [NORMAL] 通常版: 上記2つの中間。バランス型(既定値)。
 * - [ALWAYS_ON] 常時電源接続版: 充電器に繋ぎっぱなしのサーバー専用機
 *   向け。`PARTIAL_WAKE_LOCK`を保持し、画面消灯・Doze移行後もサーバー
 *   プロセスが確実に生き続けるようにする。
 *
 * **正直な開示**: これは「3電源プロファイルの最小実装」であり、Doze中の
 * ネットワークI/O制限(Androidの標準的な制約であり本アプリはこれを回避
 * しない)・バッテリー最適化ホワイトリスト登録UI・詳細な電力測定は
 * 含まない。省電力版は「積極的な常時稼働をしない」ことそのものが対応の
 * 中身であり、常時電源接続版は`WakeLock`という標準APIで「スリープさせ
 * ない」ことを実現する、という最小限のスコープ。
 */
enum class PowerProfile(val prefValue: String, val label: String, val emoji: String) {
    POWER_SAVE("power_save", "省電力", "🔋⚡️✕"), // 🔋⚡️✕ (電池+稲妻に×、省電力を示す)
    NORMAL("normal", "通常", "⚖️"), // ⚖️
    ALWAYS_ON("always_on", "常時電源接続", "🔌"); // 🔌

    companion object {
        private const val PREFS_NAME = "open_web_server_prefs"
        private const val KEY_PROFILE = "power_profile"

        fun fromPrefValue(value: String?): PowerProfile =
            values().firstOrNull { it.prefValue == value } ?: NORMAL

        fun load(context: Context): PowerProfile {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            return fromPrefValue(prefs.getString(KEY_PROFILE, null))
        }

        fun save(context: Context, profile: PowerProfile) {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            prefs.edit().putString(KEY_PROFILE, profile.prefValue).apply()
        }
    }
}
