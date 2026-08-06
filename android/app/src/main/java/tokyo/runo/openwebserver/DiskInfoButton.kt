package tokyo.runo.openwebserver

import android.os.Environment
import android.os.StatFs

/**
 * 「💾 ディスク情報を表示」ボタンのロジック(2026-08-06新設)。
 *
 * `android.os.StatFs`(標準API、root不要)で`Environment.getDataDirectory()`
 * (アプリ/システムの内部ストレージ)の総容量・使用量・空き容量を取得する。
 */
object DiskInfoButton {

    data class DiskStats(val total: Long, val used: Long, val avail: Long) {
        val usedRatio: Float
            get() = if (total > 0L) (used.toFloat() / total.toFloat()) else 0f
    }

    fun collect(): DiskStats {
        val path = Environment.getDataDirectory()
        val statFs = StatFs(path.absolutePath)
        val total = statFs.totalBytes
        val avail = statFs.availableBytes
        val used = (total - avail).coerceAtLeast(0L)
        return DiskStats(total = total, used = used, avail = avail)
    }
}
