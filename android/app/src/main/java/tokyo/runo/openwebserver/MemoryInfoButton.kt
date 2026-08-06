package tokyo.runo.openwebserver

import android.app.ActivityManager
import android.content.Context
import java.io.BufferedReader
import java.io.File
import java.io.FileReader

/**
 * 「🧠 メモリ情報を表示」ボタンのロジック(2026-08-06新設)。
 *
 * 実メモリは`ActivityManager.getMemoryInfo()`(`ActivityManager.MemoryInfo`の
 * `totalMem`/`availMem`)、仮想メモリ(スワップ)は`/proc/meminfo`の
 * `SwapTotal`/`SwapFree`(kB単位)をパースして取得する。実機によっては
 * スワップが存在しない(`SwapTotal: 0 kB`)、または`/proc/meminfo`自体が
 * 読み取れない場合があるため、そのいずれでもクラッシュせず「取得
 * できなかった」ことを正直に呼び出し側へ伝える設計にした。
 */
object MemoryInfoButton {

    /** 単位: バイト。 */
    data class MemoryStats(val total: Long, val used: Long, val avail: Long) {
        val usedRatio: Float
            get() = if (total > 0L) (used.toFloat() / total.toFloat()) else 0f
    }

    data class Result(
        val real: MemoryStats,
        /** スワップが取得できなかった場合はnull(端末にスワップが無い場合を含む)。 */
        val virtual: MemoryStats?,
        val total: MemoryStats
    )

    fun collect(context: Context): Result {
        val real = collectRealMemory(context)
        val virtual = collectSwapMemory()
        val total = if (virtual != null) {
            MemoryStats(
                total = real.total + virtual.total,
                used = real.used + virtual.used,
                avail = real.avail + virtual.avail
            )
        } else {
            // スワップが取得できない場合は実メモリのみを「合計」として扱う
            // (0扱いで誤魔化さず、正直に実メモリと同じ値にする)。
            real
        }
        return Result(real = real, virtual = virtual, total = total)
    }

    private fun collectRealMemory(context: Context): MemoryStats {
        val am = context.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        val info = ActivityManager.MemoryInfo()
        am.getMemoryInfo(info)
        val total = info.totalMem
        val avail = info.availMem
        val used = (total - avail).coerceAtLeast(0L)
        return MemoryStats(total = total, used = used, avail = avail)
    }

    /**
     * `/proc/meminfo`から`SwapTotal`/`SwapFree`(kB単位)を読み取る。
     * 読み取り失敗・値が0(スワップ未設定)の場合はnullを返す
     * (クラッシュしない、呼び出し側でその旨を表示する前提)。
     */
    private fun collectSwapMemory(): MemoryStats? {
        return try {
            var swapTotalKb: Long? = null
            var swapFreeKb: Long? = null
            BufferedReader(FileReader(File("/proc/meminfo"))).use { reader ->
                var line: String?
                while (reader.readLine().also { line = it } != null) {
                    val l = line ?: continue
                    when {
                        l.startsWith("SwapTotal:") -> swapTotalKb = parseMemInfoLineKb(l)
                        l.startsWith("SwapFree:") -> swapFreeKb = parseMemInfoLineKb(l)
                    }
                    if (swapTotalKb != null && swapFreeKb != null) break
                }
            }
            val totalKb = swapTotalKb ?: return null
            val freeKb = swapFreeKb ?: return null
            if (totalKb <= 0L) {
                // スワップ自体が無効/未設定の端末(実機で普通にあり得る)。
                return null
            }
            val total = totalKb * 1024L
            val avail = freeKb * 1024L
            val used = (total - avail).coerceAtLeast(0L)
            MemoryStats(total = total, used = used, avail = avail)
        } catch (_: Exception) {
            null
        }
    }

    /** `"SwapTotal:      123456 kB"`のような行から数値(kB)を取り出す。 */
    private fun parseMemInfoLineKb(line: String): Long? {
        return try {
            val digitsOnly = line.substringAfter(":").trim().split(Regex("\\s+"))[0]
            digitsOnly.toLong()
        } catch (_: Exception) {
            null
        }
    }

    /** バイト数をMB/GB表示に整形する簡易フォーマッタ。 */
    fun formatBytes(bytes: Long): String {
        val gb = bytes / (1024.0 * 1024.0 * 1024.0)
        return if (gb >= 1.0) {
            String.format("%.2f GB", gb)
        } else {
            val mb = bytes / (1024.0 * 1024.0)
            String.format("%.1f MB", mb)
        }
    }
}
