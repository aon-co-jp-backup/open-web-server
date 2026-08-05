package tokyo.runo.openwebserver

import android.content.Context
import android.os.Build
import android.os.storage.StorageManager
import java.util.concurrent.TimeUnit

/**
 * 接続されている外部ストレージ(マイクロSD/USB HDD・SSD/NVMe SSD等)の
 * マウントパス候補を検知する(2026-08-05新設)。
 *
 * ユーザー指示「マイクロSDや外付けUSB HDD/SSD/nVME SSDなどを簡単接続後に
 * 簡単に選択可能にして」への対応方針(ユーザー確認済み): root不要のSAF方式
 * ではなく、既存のroot化・主ストレージ切替方式(`ExternalStorageConfig`/
 * `MainActivity.showExternalStorageDialog()`)を拡張し、マウントパスの
 * 手入力を必須とせず「検知された候補から選択、または手入力」に変える。
 *
 * **正直な開示(過剰な作り込みを避ける)**: Android標準APIだけでは、
 * 接続されたストレージがマイクロSDなのかUSBマスストレージなのかNVMeなのかを
 * 確実に判別する手段が無い(`StorageVolume`はリムーバブルかどうかと
 * 人間向けの説明文字列しか持たない)。そのため、種別を確実に判別できない
 * 場合は無理にラベル付けせず「外部ストレージ候補」として一括りに表示する
 * ——完全な種別判定の実装は今回のスコープに含めない。
 */
object ExternalStorageDeviceScanner {

    /** 検知した1候補(マウントパス+人間向けラベル)。 */
    data class Candidate(
        val path: String,
        val label: String,
    )

    /**
     * `StorageManager.getStorageVolumes()`(Android標準API、root不要)経由で、
     * 非プライマリ(=内蔵ストレージ本体以外)のボリュームのマウントパスを
     * 検知する。マイクロSD・USB OTG/USB-C接続のマスストレージ・NVMe等、
     * OSがボリュームとして認識しているものはここで拾える可能性が高い。
     * 個々のボリューム取得に失敗しても例外を投げず、そのボリュームだけ
     * スキップする(1件の失敗で検知全体を諦めない)。
     */
    private fun detectViaStorageManager(context: Context): List<Candidate> {
        val results = mutableListOf<Candidate>()
        try {
            val storageManager =
                context.getSystemService(Context.STORAGE_SERVICE) as? StorageManager
                    ?: return results
            for (volume in storageManager.storageVolumes) {
                try {
                    if (!volume.isRemovable) continue // 内蔵ストレージ本体は候補から除外
                    val path = resolveVolumePath(volume) ?: continue
                    val description = try {
                        volume.getDescription(context)
                    } catch (e: Exception) {
                        null
                    }
                    val label = description?.takeIf { it.isNotBlank() }
                        ?: "外部ストレージ候補"
                    results.add(Candidate(path = path, label = label))
                } catch (e: Exception) {
                    // 個別ボリュームの取得失敗は無視して次へ。
                }
            }
        } catch (e: Exception) {
            // StorageManager自体が使えない機種でも、呼び出し元は
            // root経由の検知結果・手入力へフォールバックできる。
        }
        return results
    }

    /**
     * `android.os.storage.StorageVolume`から実マウントパスを取り出す。
     * API 30+では公開APIの`getDirectory()`、それ未満は非公開の
     * `getPath()`をリフレクション経由で呼ぶ(既存OSSにも見られる
     * 一般的な回避策)。どちらも失敗すればこのボリュームは諦める。
     */
    private fun resolveVolumePath(volume: android.os.storage.StorageVolume): String? {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            try {
                volume.directory?.let { return it.absolutePath }
            } catch (e: Exception) {
                // 下のリフレクション経路へフォールバック。
            }
        }
        return try {
            val method = volume.javaClass.getMethod("getPath")
            method.invoke(volume) as? String
        } catch (e: Exception) {
            null
        }
    }

    /**
     * root権限で`/mnt/media_rw/`配下を列挙し、`StorageManager`側で
     * 拾いきれなかった候補を補完する(root化端末専用の機種依存領域だが、
     * 実機で広く使われる慣例パスのため補助的なヒントとして採用)。
     * **正直な開示**: `su`自体は`ls /proc/partitions`のような生の
     * ブロックデバイス一覧も取得できるが、そこから実際のマウント
     * ポイントを機種非依存に導出する確立した方法が無いため、今回は
     * 慣例的なマウント先ディレクトリの列挙に留めている(過剰な作り込み
     * を避ける)。root到達不可・コマンド失敗時は例外を投げず空リストを
     * 返す(既存の「root不可時は起動を拒否する」安全設計とは別物——
     * これは検知の補助手段に過ぎず、検知失敗が起動可否に影響しない)。
     */
    private fun detectViaRootMountedMedia(): List<Candidate> {
        val results = mutableListOf<Candidate>()
        try {
            val process = ProcessBuilder(
                "su", "-c", "ls -1 /mnt/media_rw 2>/dev/null"
            ).start()
            val finished = process.waitFor(3, TimeUnit.SECONDS)
            if (!finished || process.exitValue() != 0) return results
            val output = process.inputStream.bufferedReader().readText()
            output.lineSequence()
                .map { it.trim() }
                .filter { it.isNotEmpty() }
                .forEach { name ->
                    results.add(
                        Candidate(
                            path = "/mnt/media_rw/$name",
                            label = "外部ストレージ候補(root検知)"
                        )
                    )
                }
        } catch (e: Exception) {
            // root到達不可・コマンド失敗時は空リストのまま。
        }
        return results
    }

    /**
     * 上記2経路を合わせ、パスの重複を除いた候補一覧を返す
     * (`StorageManager`経由を優先、root経由はその補完として追加)。
     * この関数はブロッキングI/O(`su`起動・ボリューム列挙)を含むため、
     * 呼び出し元は必ずUIスレッド外(`Dispatchers.IO`等)から呼ぶこと。
     */
    fun detectCandidates(context: Context): List<Candidate> {
        val seen = LinkedHashMap<String, Candidate>()
        for (candidate in detectViaStorageManager(context)) {
            seen.putIfAbsent(candidate.path, candidate)
        }
        for (candidate in detectViaRootMountedMedia()) {
            seen.putIfAbsent(candidate.path, candidate)
        }
        return seen.values.toList()
    }
}
