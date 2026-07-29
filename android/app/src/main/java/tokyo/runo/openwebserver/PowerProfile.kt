package tokyo.runo.openwebserver

import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager

/**
 * 4電源プロファイル(2026-07-24、省メモリ/省電力/通常/常時電源接続の4値)。
 *
 * **2026-07-26 再設計: 排他選択(1つだけ選ぶ)→組み合わせ選択(独立トグル、
 * 複数同時に有効化可能)へ変更**。ユーザー指示「Windows版もLINUX版も
 * Androidスマホ版も…4つのモードを組み合わせで選択出来るようにして」に
 * 対応する。この列挙型自体(4値・`prefValue`・`label`・`emoji`)は変更しない
 * ——実際に変わるのは「一度に1つしか選べない」制約を外し、[ActiveProfiles]
 * (下記)という**複数保持できる集合**を選択状態として使うようにした点。
 *
 * ## 「通常」の扱い(デスクトップ[Rust]版`power_profile.rs`と同じ設計判断)
 *
 * [NORMAL]は他の3つ(省メモリ/省電力/常時電源接続)と同時に選ぶ意味が
 * 無いため、[ActiveProfiles]内部では**独立トグルとしては扱わない**——
 * 「[MEMORY_SAVER]/[POWER_SAVE]/[ALWAYS_ON]のいずれも選ばれていない状態」
 * こそが「通常」を意味する(`ActiveProfiles.isNormal`参照)。[NORMAL]列挙値
 * 自体はホーム画面アイコン(`activity-alias`、既存の4アイコン構成を維持)・
 * UIラベル表示のためだけに引き続き存在する。
 *
 * - [MEMORY_SAVER] 省メモリ: メモリ使用量そのものを減らす施策(ログ/
 *   ヘルスチェック本文の保持バッファを縮小)。他フラグと独立した軸。
 * - [POWER_SAVE] 省電力: `WakeLock`を取得せずポーリング間隔を延ばす。
 * - [ALWAYS_ON] 常時電源接続: `PARTIAL_WAKE_LOCK`を保持しポーリング間隔を
 *   縮める。**[POWER_SAVE]と同時に有効な場合、意味論的に矛盾するため
 *   [ALWAYS_ON]が「バッテリー節約(ポーリング間隔延長)」の効果を無効化し、
 *   自身の値(間隔短縮)を優先する**——デスクトップ版`power_profile.rs`の
 *   `effective_settings()`と同じ優先順位(理由も同じ: 電源に困らない
 *   前提の機器では即応性を優先する方が実用的)。ただし[MEMORY_SAVER]の
 *   メモリ削減効果は独立軸のため、[ALWAYS_ON]の有無に関わらず適用される。
 */
enum class PowerProfile(val prefValue: String, val label: String, val emoji: String) {
    MEMORY_SAVER("memory_saver", "省メモリ", "🧠✕"), // 🧠✕ (脳=メモリに×、省メモリを示す)
    POWER_SAVE("power_save", "省電力", "🔋⚡️✕"), // 🔋⚡️✕ (電池+稲妻に×、省電力を示す)
    NORMAL("normal", "通常", "⚖️"), // ⚖️ — アイコン/ラベル表示専用、独立フラグではない(上記doc参照)
    ALWAYS_ON("always_on", "常時電源接続", "🔌"); // 🔌

    companion object {
        fun fromPrefValue(value: String?): PowerProfile =
            values().firstOrNull { it.prefValue == value } ?: NORMAL
    }
}

/**
 * 現在アクティブなプロファイルの**組み合わせ**(2026-07-26新設)。
 * [MEMORY_SAVER]/[POWER_SAVE]/[ALWAYS_ON]の任意の組み合わせを保持できる。
 * [PowerProfile.NORMAL]はここには一切含まれない(上記クラスdoc参照)。
 */
data class ActiveProfiles(
    val memorySaver: Boolean = false,
    val powerSave: Boolean = false,
    val alwaysOn: Boolean = false,
) {
    val isNormal: Boolean get() = !memorySaver && !powerSave && !alwaysOn

    /** アクティブなフラグ集合(`PowerProfile.NORMAL`は含まない)。 */
    fun activeFlags(): List<PowerProfile> {
        val out = mutableListOf<PowerProfile>()
        if (memorySaver) out.add(PowerProfile.MEMORY_SAVER)
        if (powerSave) out.add(PowerProfile.POWER_SAVE)
        if (alwaysOn) out.add(PowerProfile.ALWAYS_ON)
        return out
    }

    /** 表示用ラベル(空なら`["通常"]`)。 */
    fun displayLabels(): List<String> {
        val flags = activeFlags().map { it.label }
        return if (flags.isEmpty()) listOf(PowerProfile.NORMAL.label) else flags
    }

    /**
     * ホーム画面アイコン(`activity-alias`)選択のための「代表プロファイル」
     * 1つを、優先順位で決める(2026-07-26追加、複数同時選択に対応する
     * ために新設——`applyLauncherIcon`参照)。
     *
     * **優先順位の理由(コメントで明記、ユーザー指示通り)**:
     * 省メモリ > 省電力 > 常時電源接続 > 通常(何も選ばれていない場合の
     * 既定)。低メモリ環境は端末の動作自体に最も直接的な制約(OOM Kill等)
     * を及ぼしうるため最優先で視認できるようにし、次に省電力(バッテリー
     * 影響)、常時電源接続(充電中提示)の順とした。将来この優先順位を
     * 変える場合はここを書き換えるだけでよい(単一箇所に集約)。
     */
    fun representativeForIcon(): PowerProfile = when {
        memorySaver -> PowerProfile.MEMORY_SAVER
        powerSave -> PowerProfile.POWER_SAVE
        alwaysOn -> PowerProfile.ALWAYS_ON
        else -> PowerProfile.NORMAL
    }

    /** DDNS/Rust側APIと同じ`prefValue`文字列配列(空なら通常=空配列)。 */
    fun toPrefValues(): List<String> = activeFlags().map { it.prefValue }

    companion object {
        fun fromPrefValues(values: Collection<String>): ActiveProfiles = ActiveProfiles(
            memorySaver = values.contains(PowerProfile.MEMORY_SAVER.prefValue),
            powerSave = values.contains(PowerProfile.POWER_SAVE.prefValue),
            alwaysOn = values.contains(PowerProfile.ALWAYS_ON.prefValue),
        )
    }
}

/**
 * [ActiveProfiles]の永続化・ホーム画面アイコン反映(2026-07-26新設、旧
 * `PowerProfile.save()`/`PowerProfile.load()`を置き換える)。
 */
object PowerProfileStore {
    private const val PREFS_NAME = "open_web_server_prefs"
    private const val KEY_MEMORY_SAVER = "flag_memory_saver"
    private const val KEY_POWER_SAVE = "flag_power_save"
    private const val KEY_ALWAYS_ON = "flag_always_on"

    fun load(context: Context): ActiveProfiles {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        return ActiveProfiles(
            memorySaver = prefs.getBoolean(KEY_MEMORY_SAVER, false),
            powerSave = prefs.getBoolean(KEY_POWER_SAVE, false),
            alwaysOn = prefs.getBoolean(KEY_ALWAYS_ON, false),
        )
    }

    fun save(context: Context, profiles: ActiveProfiles) {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        prefs.edit()
            .putBoolean(KEY_MEMORY_SAVER, profiles.memorySaver)
            .putBoolean(KEY_POWER_SAVE, profiles.powerSave)
            .putBoolean(KEY_ALWAYS_ON, profiles.alwaysOn)
            .apply()
        applyLauncherIcon(context, profiles)
    }

    /**
     * ホーム画面上のランチャーアイコンを、[ActiveProfiles.representativeForIcon]
     * が選んだ1つの`activity-alias`だけ`COMPONENT_ENABLED_STATE_ENABLED`にし、
     * 他3つを`COMPONENT_ENABLED_STATE_DISABLED`にする(2026-07-24追加、
     * 2026-07-26に複数選択対応——「代表アイコン」方式へ変更、上記
     * `representativeForIcon()`のdoc参照)。
     */
    fun applyLauncherIcon(context: Context, profiles: ActiveProfiles) {
        val pm = context.packageManager
        val pkg = context.packageName
        val aliasByProfile = mapOf(
            PowerProfile.MEMORY_SAVER to "$pkg.LauncherMemorySaver",
            PowerProfile.POWER_SAVE to "$pkg.LauncherPowerSave",
            PowerProfile.NORMAL to "$pkg.LauncherNormal",
            PowerProfile.ALWAYS_ON to "$pkg.LauncherAlwaysOn",
        )
        val representative = profiles.representativeForIcon()
        for ((p, aliasClass) in aliasByProfile) {
            val state = if (p == representative) {
                PackageManager.COMPONENT_ENABLED_STATE_ENABLED
            } else {
                PackageManager.COMPONENT_ENABLED_STATE_DISABLED
            }
            pm.setComponentEnabledSetting(
                ComponentName(pkg, aliasClass),
                state,
                PackageManager.DONT_KILL_APP,
            )
        }
    }
}
