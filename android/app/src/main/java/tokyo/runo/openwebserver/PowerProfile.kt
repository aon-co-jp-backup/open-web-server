package tokyo.runo.openwebserver

import android.content.Context

/**
 * 4電源プロファイル(2026-07-24、ユーザー指示で「省電力」と「省メモリ」を
 * 明確に別軸として追加)。
 *
 * - [MEMORY_SAVER] 省メモリ版: メモリ使用量そのものを減らす施策(画像/
 *   データキャッシュの上限を厳しく下げる・バックグラウンド先読みを
 *   行わない・保持するログ/履歴バッファを縮小する)が中身。[POWER_SAVE]
 *   とは異なる軸の最適化であることを`MainActivity`側の具体的な数値差
 *   (`memoryCacheLimitBytes`/`logBufferMaxLines`等)で示す。
 * - [POWER_SAVE] 省電力版: バックグラウンドでの常時稼働を避け、Android
 *   Doze/App Standbyに逆らわない(=`WakeLock`を一切取得しない)。ポーリング
 *   間隔を延ばすことが中身で、メモリ使用量自体は積極的に削らない
 *   (=[MEMORY_SAVER]と重複しない別軸)。
 * - [NORMAL] 通常版: バランス型(既定値)。
 * - [ALWAYS_ON] 常時電源接続版: 充電器に繋ぎっぱなしのサーバー専用機
 *   向け。`PARTIAL_WAKE_LOCK`を保持し、画面消灯・Doze移行後もサーバー
 *   プロセスが確実に生き続けるようにする。CPU+GPU(外付け含む想定)+
 *   NPUハードウェアアクセラレーター対応(`OPEN_WEB_SERVER_ACCEL_BACKEND`
 *   環境変数連携)。
 *
 * **正直な開示**: これは「4電源プロファイルの最小実装」であり、Doze中の
 * ネットワークI/O制限(Androidの標準的な制約であり本アプリはこれを回避
 * しない)・バッテリー最適化ホワイトリスト登録UI・詳細な電力/メモリ測定は
 * 含まない。省電力版は「積極的な常時稼働をしない」こと、省メモリ版は
 * 「保持するデータ量そのものを絞る」こと、常時電源接続版は`WakeLock`と
 * いう標準APIで「スリープさせない」ことを実現する、という最小限の
 * スコープ。外付けGPUについては、Android自体が外付けGPUを一般的に
 * サポートする標準APIを持たないため、Android側での複雑な検出実装は
 * 行わず、環境変数`OPEN_WEB_SERVER_ACCEL_BACKEND`の値がRust側へ渡る
 * 既存の仕組みのみで対応する(過剰実装回避)。
 */
enum class PowerProfile(val prefValue: String, val label: String, val emoji: String) {
    MEMORY_SAVER("memory_saver", "省メモリ", "🧠✕"), // 🧠✕ (脳=メモリに×、省メモリを示す)
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
