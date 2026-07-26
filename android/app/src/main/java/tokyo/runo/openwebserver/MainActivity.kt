package tokyo.runo.openwebserver

import android.content.ActivityNotFoundException
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.Uri
import android.os.Bundle
import android.os.PowerManager
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import java.io.BufferedReader
import java.io.File
import java.io.InputStreamReader
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * open-web-server Android版シェル(2026-07-23着手、2026-07-24に4電源
 * プロファイル[省メモリ/省電力/通常/常時電源接続]対応・open-easy-web
 * 連携導線を追加)。
 *
 * このActivity自体はサーバー機能を一切実装しない。クロスコンパイル済みの
 * `open-web-server`ネイティブ実行ファイル(`jniLibs/<abi>/libopenwebserver.so`
 * として同梱——nativeLibraryDir配下に配置することでAndroid 10+のW^X制約下でも
 * 実行可能にする、Termux等が使う既知の手法)を`ProcessBuilder`で起動し、
 * 起動後に自分自身へ`GET /healthz`を投げて実際に応答することを画面上で確認できる
 * ようにする。
 *
 * スコープ(意図的に今回含めない、詳細はリポジトリ`CLAUDE.md`のHANDOFF節参照):
 * フォアグラウンドサービス化、APK署名・配布、Doze中のネットワークI/O制限自体の
 * 回避(標準の制約であり本アプリは回避しない)。
 */
class MainActivity : AppCompatActivity() {

    companion object {
        const val EXTRA_PROFILE = "profile"

        /**
         * サーバーのbindポート(2026-07-24、`DdnsSetupActivity`からも
         * 同じローカルポートへ管理APIを叩く必要があるため`companion
         * object`定数として公開)。
         */
        const val SERVER_PORT = 18099
    }

    private var serverProcess: Process? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private val bindPort = 18099

    /**
     * 定期ヘルスチェックのポーリング間隔(2026-07-24追加、ユーザー指示
     * 「省電力版は実際に省電力になるようにして」の具体的施策の一つ)。
     * 省電力版は間隔を大きく延ばし(Doze/App Standbyへの影響を最小化)、
     * 常時電源接続版は短い間隔で即応性を優先する、という実際の挙動差を
     * 持たせる。
     */
    private fun healthPollIntervalMs(profile: PowerProfile): Long = when (profile) {
        PowerProfile.POWER_SAVE -> 5 * 60_000L // 5分
        // 省メモリ版はポーリング頻度自体は通常版と同じにする(ポーリング
        // 間隔の延長は「省電力」の施策軸であり、「省メモリ」の施策軸
        // [下記memoryCacheLimitBytes/logBufferMaxLines等]とは別物として
        // 明確に区別する、ユーザー指示2026-07-24)。
        PowerProfile.MEMORY_SAVER -> 60_000L // 1分(通常と同じ)
        PowerProfile.NORMAL -> 60_000L // 1分
        PowerProfile.ALWAYS_ON -> 5_000L // 5秒
    }

    /**
     * 省メモリ版の具体的施策その1(2026-07-24追加、ユーザー指示
     * 「省電力と省メモリは別軸として区別すること」への対応)。
     * ログ画面(`logText`)に保持する行数の上限——省メモリ版は履歴
     * バッファを大きく縮小し、それ以外は緩やかな上限とする。実際に
     * `StringBuilder`の内容を`appendLine`のたびにこの行数へ切り詰める
     * ことで、長時間稼働時のメモリ使用量差として実際に効果を持つ。
     */
    private fun logBufferMaxLines(profile: PowerProfile): Int = when (profile) {
        PowerProfile.MEMORY_SAVER -> 40
        PowerProfile.POWER_SAVE, PowerProfile.NORMAL -> 400
        PowerProfile.ALWAYS_ON -> 2000
    }

    /**
     * 省メモリ版の具体的施策その2。ヘルスチェックの結果本文
     * (`pollHealthz`が記録する`body`)を保持する最大バイト数——省メモリ
     * 版は長いレスポンスボディを大きく切り詰めて保持しないようにする
     * (バックグラウンドでの先読み・プリフェッチは元々本アプリに存在
     * しないため、キャッシュ/バッファサイズの縮小という形で「メモリ
     * 使用量を実際に減らす」施策を実装する)。
     */
    private fun healthBodyPreviewMaxChars(profile: PowerProfile): Int = when (profile) {
        PowerProfile.MEMORY_SAVER -> 64
        PowerProfile.POWER_SAVE, PowerProfile.NORMAL -> 512
        PowerProfile.ALWAYS_ON -> 4096
    }

    /**
     * ログバッファを`logBufferMaxLines(currentProfile)`件までに切り詰める
     * (先頭[古い行]から破棄)。`StringBuilder`をまるごと作り直す単純な
     * 実装だが、呼び出し頻度はヘルスチェックのポーリング間隔と同程度
     * (最速でも常時電源接続版の5秒に1回)のため実用上問題にならない。
     */
    private fun trimLogBuffer(log: StringBuilder) {
        val maxLines = logBufferMaxLines(currentProfile)
        val lines = log.lines()
        if (lines.size > maxLines) {
            val trimmed = lines.takeLast(maxLines)
            log.setLength(0)
            log.append(trimmed.joinToString("\n"))
            if (trimmed.isNotEmpty()) log.appendLine()
        }
    }

    /**
     * ハードウェアアクセラレーター(CPU+GPU+NPU)対応の指示
     * (`open-web-server-wire::accel::AccelBackend`、環境変数
     * `OPEN_WEB_SERVER_ACCEL_BACKEND`、`state.rs::accel_backend_from_env()`
     * が解釈)。2026-07-24(続き)、ユーザー指示「実際に検出ロジックを
     * 実装してほしい」を受け、`"hardware_accelerator"`固定値から
     * `HardwareAccelDetector`が実際に検出したGPU/NPU/外部ディスプレイ
     * 情報を反映した値へ変更した(常時電源接続版のみ)。省電力/省メモリ/
     * 通常は引き続き明示的に`cpu`を指定する(ハードウェア検出自体は
     * 常時電源接続版専用のスコープのまま)。**正直な開示**: 本体(Rust)側は
     * 現時点でこの値を`AppState.accel_backend`に保持・起動ログへ出力
     * するのみで、実際の圧縮/暗号化処理へは未配線(Gpu/Npu/
     * HardwareAcceleratorはいずれも常にCpu実装にフォールバックする、
     * `open-web-server`側CLAUDE.md HANDOFF参照)。このAndroid側の指定は
     * 将来配線された際に効果を持つようになる先取り実装であり、現時点で
     * 実際の消費電力・性能へ影響するのは電源プロファイルによる
     * WakeLock有無とポーリング間隔の差のみ。
     */
    private fun accelBackendEnvValue(profile: PowerProfile): String = when (profile) {
        PowerProfile.ALWAYS_ON -> hardwareDetection.toAccelBackendEnvValue()
        PowerProfile.MEMORY_SAVER, PowerProfile.POWER_SAVE, PowerProfile.NORMAL -> "cpu"
    }

    /**
     * ハードウェア検出結果(2026-07-24(続き)新設)。`onCreate`内で1度だけ
     * 検出し、サーバー起動時の環境変数生成・検出画面表示の両方で使い回す
     * (毎回EGLコンテキストを作り直すコストを避けるため)。
     */
    private lateinit var hardwareDetection: HardwareAccelDetector.DetectionResult

    private var healthPollJob: Job? = null
    private var powerConnectionReceiver: BroadcastReceiver? = null

    /**
     * open-easy-webのドメイン設定ウィザードを開くためのデフォルトURL。
     * 「open-easy-webとSETのopen-web-server」という位置づけ(ユーザー
     * 指示、2026-07-24)を踏まえ、同一端末/同一LAN上で
     * `python -m http.server 8080`等で配信されているopen-easy-webへの
     * 導線を提供する——このAndroidアプリ自体はopen-easy-webを同梱しない
     * (別プロジェクト・別デプロイ、過剰実装を避ける)。
     */
    private val openEasyWebUrl = "http://127.0.0.1:8080"

    private lateinit var currentProfile: PowerProfile

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        currentProfile = resolveProfile()
        PowerProfile.save(this, currentProfile)
        hardwareDetection = HardwareAccelDetector.detect(this)

        val statusText = findViewById<TextView>(R.id.statusText)
        val logText = findViewById<TextView>(R.id.logText)
        val startButton = findViewById<Button>(R.id.startButton)
        val openEasyWebButton = findViewById<Button>(R.id.openEasyWebButton)
        val changeProfileButton = findViewById<Button>(R.id.changeProfileButton)
        val ddnsSetupButton = findViewById<Button>(R.id.ddnsSetupButton)
        val hardwareInfoButton = findViewById<Button>(R.id.hardwareInfoButton)

        hardwareInfoButton.setOnClickListener {
            showHardwareInfoDialog()
        }

        statusText.text =
            "open-web-server [${currentProfile.emoji} ${currentProfile.label}モード] (not started)"

        startButton.setOnClickListener {
            startButton.isEnabled = false
            CoroutineScope(Dispatchers.Main).launch {
                val log = StringBuilder()
                log.appendLine("profile: ${currentProfile.label} (${currentProfile.prefValue})")
                statusText.text = "[${currentProfile.emoji} ${currentProfile.label}] starting..."
                val startResult = withContext(Dispatchers.IO) { startServerProcess(log) }
                if (!startResult) {
                    statusText.text = "[${currentProfile.emoji} ${currentProfile.label}] failed to start (see log)"
                    logText.text = log.toString()
                    startButton.isEnabled = true
                    return@launch
                }

                applyProfilePowerBehavior(log)

                // ネイティブプロセスがリスンし始めるまで少し待ってからヘルス
                // チェックする(即座に叩くとACCEPT前でconnection refusedになり得る)。
                val healthOk = withContext(Dispatchers.IO) { pollHealthz(log) }
                statusText.text = if (healthOk) {
                    "[${currentProfile.emoji} ${currentProfile.label}] RUNNING: GET /healthz responded 200"
                } else {
                    "[${currentProfile.emoji} ${currentProfile.label}] started, but /healthz did not respond (see log)"
                }
                logText.text = log.toString()
                startButton.isEnabled = true

                if (healthOk) {
                    startPeriodicHealthPoll(statusText)
                }
            }
        }

        openEasyWebButton.setOnClickListener {
            openEasyWeb()
        }

        // **2026-07-26変更(ユーザー指示「Windows版もLINUX版もAndroidスマホ
        // 版も全てのバージョンで省メモリと省電力モードを途中からでも選択
        // 出来るようにして」)**: 以前は`ProfileSelectActivity`(LAUNCHER)へ
        // 遷移して`finish()`するのみで、これは事実上「アプリを終了して次回
        // 起動時に新プロファイルを選び直す」フロー——`serverProcess`ごと
        // 終了させてしまうため、稼働中のネイティブサーバーを止めずに
        // プロファイルだけ切り替えることはできなかった。`showLiveProfile
        // SwitchDialog()`に差し替え、稼働中のプロセス・Activityを維持した
        // ままプロファイルを切り替えられるようにした(再起動が必要な項目は
        // 正直にダイアログ内で案内する、下記`switchProfileLive()`のdoc参照)。
        changeProfileButton.setOnClickListener {
            showLiveProfileSwitchDialog()
        }

        ddnsSetupButton.setOnClickListener {
            startActivity(Intent(this, DdnsSetupActivity::class.java))
        }

        registerPowerConnectionReceiver()
    }

    /**
     * 電源の抜き差しを監視する(2026-07-24追加、ユーザー指示「常時電源
     * 接続版は…電源から外したら自動で、デフォルトは省電力モード、
     * もしくは通常版に切り替えますか?と質問して切り替える」)。
     *
     * - 常時電源接続版の実行中に`ACTION_POWER_DISCONNECTED`を受信したら、
     *   「省電力モードに切り替えますか?それとも通常モードのままに
     *   しますか?」とダイアログで質問する(既定の推奨選択肢は省電力)。
     * - 省電力/通常版の実行中に`ACTION_POWER_CONNECTED`を受信したら、
     *   常時電源接続版に戻すかを尋ねる(電源再接続時の導線)。
     *
     * ダイアログは`this`(Activity)がフォアグラウンドにある前提
     * (`registerReceiver`はActivityのライフサイクルに紐づけて
     * `onDestroy`で解除する、バックグラウンドサービス化は今回の
     * スコープ外)。
     */
    private fun registerPowerConnectionReceiver() {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                when (intent.action) {
                    Intent.ACTION_POWER_DISCONNECTED -> onPowerDisconnected()
                    Intent.ACTION_POWER_CONNECTED -> onPowerConnected()
                }
            }
        }
        powerConnectionReceiver = receiver
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_POWER_DISCONNECTED)
            addAction(Intent.ACTION_POWER_CONNECTED)
        }
        registerReceiver(receiver, filter)
    }

    /**
     * 電源切断時の確認ダイアログ(2026-07-24、2択→3択へ変更、ユーザー
     * 指示「省電力版に切り替えますか?省メモリ版に切り替えますか?もしくは
     * 普通版に切り替えますか?」)。`AlertDialog`のボタンは実用上3つまでが
     * 適切なため、`setPositiveButton`/`setNegativeButton`/
     * `setNeutralButton`の3ボタン構成を採用(3択リストダイアログではなく
     * こちらを選んだ理由: 既存の2択ボタン実装からの変更が最小で済み、
     * 各選択肢の推奨度をボタンの目立たせ方[Positiveを既定推奨として先頭
     * 表示]で表現しやすいため)。既定推奨(強調表示)は既存方針を踏襲し
     * 「省電力」を第一候補のまま維持する。
     */
    private fun onPowerDisconnected() {
        if (currentProfile != PowerProfile.ALWAYS_ON) return
        if (isFinishing || isDestroyed) return
        AlertDialog.Builder(this)
            .setTitle("電源が外れました")
            .setMessage(
                "常時電源接続モードで動作中に電源が外れました。\n" +
                    "省電力版に切り替えますか?省メモリ版に切り替えますか?\n" +
                    "もしくは普通版(通常版)に切り替えますか?\n" +
                    "(推奨: 省電力版)"
            )
            .setPositiveButton("省電力版へ切替") { _, _ ->
                switchProfileAndRestart(PowerProfile.POWER_SAVE)
            }
            .setNeutralButton("省メモリ版へ切替") { _, _ ->
                switchProfileAndRestart(PowerProfile.MEMORY_SAVER)
            }
            .setNegativeButton("普通版(通常版)のままにする") { _, _ ->
                switchProfileAndRestart(PowerProfile.NORMAL)
            }
            .setCancelable(false)
            .show()
    }

    private fun onPowerConnected() {
        if (currentProfile == PowerProfile.ALWAYS_ON) return
        if (isFinishing || isDestroyed) return
        AlertDialog.Builder(this)
            .setTitle("電源が接続されました")
            .setMessage("常時電源接続モード(ハードウェアアクセラレーター対応)に切り替えますか?")
            .setPositiveButton("常時電源接続へ切替") { _, _ ->
                switchProfileAndRestart(PowerProfile.ALWAYS_ON)
            }
            .setNegativeButton("このままにする", null)
            .show()
    }

    /**
     * プロファイルを保存し、稼働中のサーバープロセスを終了・
     * `MainActivity`を再起動して新プロファイルで再起動させる
     * (WakeLock取得の有無・ポーリング間隔・アクセラレーター指定は
     * プロセス起動時に確定する値のため、切替には再起動が必要)。
     */
    private fun switchProfileAndRestart(newProfile: PowerProfile) {
        PowerProfile.save(this, newProfile)
        Toast.makeText(
            this,
            "${newProfile.emoji} ${newProfile.label}モードへ切り替えます",
            Toast.LENGTH_SHORT
        ).show()
        val intent = Intent(this, MainActivity::class.java)
        intent.putExtra(EXTRA_PROFILE, newProfile.prefValue)
        startActivity(intent)
        finish()
    }

    /**
     * 稼働中のサーバープロセス・Activityを終了させずにプロファイルを
     * 切り替える(2026-07-26新設、ユーザー指示「途中からでも選択出来る
     * ようにして」への対応、`switchProfileAndRestart()`との違いはdoc
     * 参照)。押されたボタン(`changeProfileButton`)から呼ばれる。
     *
     * 4択のうち現在のプロファイルにはチェックマークを付け、選択すると
     * 即座に`switchProfileLive()`を呼ぶ(確認ダイアログを二重に出さない
     * 簡潔なUXとした)。
     */
    private fun showLiveProfileSwitchDialog() {
        val profiles = PowerProfile.values()
        val labels = profiles.map { p ->
            val mark = if (p == currentProfile) "✓ " else "   "
            "$mark${p.emoji} ${p.label}"
        }.toTypedArray()
        AlertDialog.Builder(this)
            .setTitle("電源プロファイルを切り替え(再起動不要)")
            .setItems(labels) { _, which ->
                switchProfileLive(profiles[which])
            }
            .setNegativeButton("キャンセル", null)
            .show()
    }

    /**
     * プロファイルを、アプリ・サーバープロセスを再起動せずに切り替える
     * (2026-07-26新設)。`switchProfileAndRestart()`(電源切断/再接続
     * ダイアログ用、既存のまま維持)とは異なり、こちらは`MainActivity`を
     * 終了せず`serverProcess`も殺さない——「途中からでも選択出来るように
     * して」というユーザー指示の中核。
     *
     * **実際に即座に反映される項目**:
     * - `PowerProfile.save()`(永続化+ホーム画面アイコンの動的切替、
     *   `applyLauncherIcon()`参照)。
     * - `WakeLock`の取得/解放(`applyProfilePowerBehaviorLive()`、下記)。
     * - ヘルスチェックのポーリング間隔(`startPeriodicHealthPoll()`が
     *   毎イテレーション`currentProfile`を読み直すよう上で改修済みのため、
     *   次の1回の待機から新間隔になる)。
     * - ログ保持行数・ヘルスチェック本文保持サイズ(`logBufferMaxLines`/
     *   `healthBodyPreviewMaxChars`は元々`currentProfile`を直接参照する
     *   実装だったため、これも改修不要で即座に反映される)。
     *
     * **正直な開示・再起動が必要なまま残る項目**: `OPEN_WEB_SERVER_
     * ACCEL_BACKEND`環境変数は`ProcessBuilder`がネイティブプロセスを
     * 起動する瞬間にしか渡せないため、常時電源接続版⇔他プロファイル間の
     * ハードウェアアクセラレータ指定の切替自体は、ネイティブサーバー
     * プロセスの再起動(`serverProcess`の再起動、Activity自体は再起動
     * しない)が必要なまま——この制約はダイアログ内で正直に案内する。
     */
    private fun switchProfileLive(newProfile: PowerProfile) {
        if (newProfile == currentProfile) return
        val previousProfile = currentProfile
        currentProfile = newProfile
        PowerProfile.save(this, newProfile)

        val log = StringBuilder()
        applyProfilePowerBehaviorLive(previousProfile, log)

        Toast.makeText(
            this,
            "${newProfile.emoji} ${newProfile.label}モードへ切替(再起動なし)",
            Toast.LENGTH_SHORT
        ).show()

        val statusText = findViewById<TextView>(R.id.statusText)
        if (serverProcess?.isAlive == true) {
            statusText.text = "[${newProfile.emoji} ${newProfile.label}] RUNNING (switched live, no restart)"
        } else {
            statusText.text =
                "open-web-server [${newProfile.emoji} ${newProfile.label}モード] (not started)"
        }

        if (log.isNotEmpty()) {
            android.util.Log.i("open-web-server", log.toString().trim())
        }

        if (accelBackendEnvValue(previousProfile) != accelBackendEnvValue(newProfile)
            && serverProcess?.isAlive == true
        ) {
            Toast.makeText(
                this,
                "ハードウェアアクセラレータ指定の変更は、次回サーバー再起動時に反映されます",
                Toast.LENGTH_LONG
            ).show()
        }
    }

    /**
     * `switchProfileLive()`専用の電源管理適用(`applyProfilePowerBehavior()`
     * はプロセス起動時にログへ書くだけの一発物だったため、稼働中に
     * 呼び直せる形へ別関数として新設した——`WakeLock`の実際の取得/解放を
     * 即座に行う点が中身)。
     */
    private fun applyProfilePowerBehaviorLive(previousProfile: PowerProfile, log: StringBuilder) {
        val needsWakeLock = currentProfile == PowerProfile.ALWAYS_ON
        val hasWakeLock = wakeLock?.isHeld == true

        if (needsWakeLock && !hasWakeLock) {
            try {
                val pm = getSystemService(POWER_SERVICE) as PowerManager
                val lock = pm.newWakeLock(
                    PowerManager.PARTIAL_WAKE_LOCK,
                    "OpenWebServer::AlwaysOnWakeLock"
                )
                lock.acquire()
                wakeLock = lock
                log.appendLine("power: acquired PARTIAL_WAKE_LOCK live (switched to always-on profile)")
            } catch (e: Exception) {
                log.appendLine("power: failed to acquire WakeLock live: ${e.message}")
            }
        } else if (!needsWakeLock && hasWakeLock) {
            wakeLock?.release()
            wakeLock = null
            log.appendLine(
                "power: released WakeLock live (switched away from always-on profile, " +
                    "was: ${previousProfile.label})"
            )
        }
    }

    /**
     * `activity-alias`(専用ホーム画面アイコン)経由なら`Intent.action`から、
     * `ProfileSelectActivity`経由なら`EXTRA_PROFILE`から、どちらでも無い
     * (直接`MainActivity`が再利用された等)場合は前回保存値から、
     * プロファイルを決定する。
     */
    private fun resolveProfile(): PowerProfile {
        return when (intent?.action) {
            "tokyo.runo.openwebserver.LAUNCH_MEMORY_SAVER" -> PowerProfile.MEMORY_SAVER
            "tokyo.runo.openwebserver.LAUNCH_POWER_SAVE" -> PowerProfile.POWER_SAVE
            "tokyo.runo.openwebserver.LAUNCH_NORMAL" -> PowerProfile.NORMAL
            "tokyo.runo.openwebserver.LAUNCH_ALWAYS_ON" -> PowerProfile.ALWAYS_ON
            else -> {
                val extra = intent?.getStringExtra(EXTRA_PROFILE)
                if (extra != null) PowerProfile.fromPrefValue(extra) else PowerProfile.load(this)
            }
        }
    }

    /**
     * プロファイルごとの電源管理の中身そのもの。
     * - 省電力/通常: `WakeLock`を一切取得しない(=Android Doze/App
     *   Standbyに逆らわない、これが「省電力対応」の実体)。
     * - 常時電源接続: `PARTIAL_WAKE_LOCK`を保持し、画面消灯後もCPUを
     *   スリープさせない(充電しっぱなしのサーバー専用機を想定)。
     */
    private fun applyProfilePowerBehavior(log: StringBuilder) {
        when (currentProfile) {
            PowerProfile.ALWAYS_ON -> {
                try {
                    val pm = getSystemService(POWER_SERVICE) as PowerManager
                    val lock = pm.newWakeLock(
                        PowerManager.PARTIAL_WAKE_LOCK,
                        "OpenWebServer::AlwaysOnWakeLock"
                    )
                    lock.acquire()
                    wakeLock = lock
                    log.appendLine("power: acquired PARTIAL_WAKE_LOCK (always-on profile)")
                } catch (e: Exception) {
                    log.appendLine("power: failed to acquire WakeLock: ${e.message}")
                }
            }
            PowerProfile.POWER_SAVE -> {
                log.appendLine("power: no WakeLock acquired (power-save profile, Doze-friendly)")
            }
            PowerProfile.MEMORY_SAVER -> {
                // 「省電力」とは別軸: WakeLockの有無ではなく、ログ保持行数
                // (`logBufferMaxLines`)・ヘルスチェック本文の保持サイズ
                // (`healthBodyPreviewMaxChars`)を大きく絞ることでメモリ
                // 使用量そのものを減らす、というのがこのプロファイルの
                // 実体。詳細な数値差は各関数のdoc参照。
                log.appendLine(
                    "memory: log buffer capped at ${logBufferMaxLines(currentProfile)} lines, " +
                        "health body preview capped at ${healthBodyPreviewMaxChars(currentProfile)} chars " +
                        "(memory-saver profile, no background prefetch/no large caches)"
                )
            }
            PowerProfile.NORMAL -> {
                log.appendLine("power: no WakeLock acquired (normal profile)")
            }
        }
    }

    /**
     * ハードウェア検出結果の表示ダイアログ(2026-07-24(続き)新設)。
     * 検出画面(設定画面や常時電源接続選択時)にGPU名・NPU利用可否・
     * 外部ディスプレイ有無をユーザーへ表示する要件への対応。
     */
    private fun showHardwareInfoDialog() {
        AlertDialog.Builder(this)
            .setTitle("検出したハードウェア情報 / Detected Hardware Info")
            .setMessage(
                hardwareDetection.toHumanReadableSummary() +
                    "\n\n常時電源接続モードで起動した場合、OPEN_WEB_SERVER_ACCEL_BACKEND=\"" +
                    hardwareDetection.toAccelBackendEnvValue() +
                    "\" として渡されます。" +
                    "\nWhen started in Always-On mode, OPEN_WEB_SERVER_ACCEL_BACKEND=\"" +
                    hardwareDetection.toAccelBackendEnvValue() +
                    "\" will be passed."
            )
            .setPositiveButton("閉じる / Close", null)
            .show()
    }

    private fun openEasyWeb() {
        try {
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse(openEasyWebUrl))
            startActivity(intent)
        } catch (e: ActivityNotFoundException) {
            Toast.makeText(this, "ブラウザが見つかりません: $openEasyWebUrl", Toast.LENGTH_LONG).show()
        }
    }

    private fun startServerProcess(log: StringBuilder): Boolean {
        return try {
            // `nativeLibraryDir`配下はAndroidが自動でAPKから展開・配置する、
            // W^X制約下でも実行可能な数少ない領域。
            val binaryPath = File(applicationInfo.nativeLibraryDir, "libopenwebserver.so")
            log.appendLine("binary path: ${binaryPath.absolutePath}")
            log.appendLine("binary exists: ${binaryPath.exists()}")
            if (!binaryPath.exists()) {
                log.appendLine("ERROR: native binary not found — was the app built with jniLibs populated by cargo-ndk?")
                return false
            }

            val pb = ProcessBuilder(binaryPath.absolutePath)
            pb.directory(filesDir)
            pb.environment()["OPEN_WEB_SERVER_BIND"] = "127.0.0.1:$bindPort"
            pb.environment()["OPEN_WEB_SERVER_ACCEL_BACKEND"] = accelBackendEnvValue(currentProfile)
            log.appendLine("accel backend requested: ${accelBackendEnvValue(currentProfile)}")

            // DuckDNS DDNS設定画面(2026-07-24追加)からRust側管理API
            // (`/admin/ddns/*`)を叩けるようにするため、`SecureDdnsStore`に
            // 保存済みの管理トークンをこのプロセスの`OPEN_WEB_SERVER_
            // ADMIN_TOKEN`として渡す(未設定ならRust側は無認証のまま起動、
            // 既存の後方互換動作)。トークン自体はログへ出力しない。
            SecureDdnsStore.getAdminToken(this)?.let { token ->
                pb.environment()["OPEN_WEB_SERVER_ADMIN_TOKEN"] = token
                log.appendLine("admin token: configured (value not logged)")
            }
            pb.redirectErrorStream(true)
            val process = pb.start()
            serverProcess = process

            // stdoutを非同期で読み続けてログ画面に反映する(バッファが
            // 詰まってプロセスがブロックするのを避けるため、専用スレッドで
            // 継続的にdrainする)。
            Thread {
                try {
                    BufferedReader(InputStreamReader(process.inputStream)).use { reader ->
                        var line: String?
                        while (reader.readLine().also { line = it } != null) {
                            android.util.Log.i("open-web-server", line ?: "")
                        }
                    }
                } catch (_: Exception) {
                    // プロセス終了時にストリームが閉じるのは正常系。
                }
            }.start()

            log.appendLine("process started (alive=${process.isAlive})")
            true
        } catch (e: Exception) {
            log.appendLine("ERROR launching process: ${e}")
            false
        }
    }

    /**
     * 起動後の継続的な死活監視(2026-07-24追加)。プロファイルごとに
     * 間隔を変える(`healthPollIntervalMs`)ことが「省電力版が実際に
     * 省電力になる」施策そのもの——省電力版はこのループの頻度自体を
     * 大きく落とし、CPU/無線を起こす回数を最小化する。常時電源接続版は
     * 短い間隔で即応性を優先する。
     */
    private fun startPeriodicHealthPoll(statusText: TextView) {
        healthPollJob?.cancel()
        healthPollJob = CoroutineScope(Dispatchers.Main).launch {
            while (isActive) {
                // **2026-07-26変更(ユーザー指示「途中からでもプロファイル
                // 切替できるように」対応)**: 以前はループ開始時に
                // `healthPollIntervalMs(currentProfile)`を1度だけ評価して
                // `intervalMs`へ固定していたため、`switchProfileLive()`で
                // `currentProfile`を書き換えてもこのループの間隔には次回
                // ループ起動(=サーバー再起動)まで反映されなかった。
                // 毎イテレーションで`currentProfile`を読み直す形に変更し、
                // アプリ再起動はもちろんサーバープロセス再起動も無しに、
                // 次の1回の待機から新しい間隔が反映されるようにした。
                val intervalMs = healthPollIntervalMs(currentProfile)
                delay(intervalMs)
                val ok = withContext(Dispatchers.IO) {
                    try {
                        val url = URL("http://127.0.0.1:$bindPort/healthz")
                        val conn = url.openConnection() as HttpURLConnection
                        conn.connectTimeout = 1000
                        conn.readTimeout = 1000
                        val code = conn.responseCode
                        conn.disconnect()
                        code == 200
                    } catch (_: Exception) {
                        false
                    }
                }
                statusText.text = if (ok) {
                    "[${currentProfile.emoji} ${currentProfile.label}] RUNNING " +
                        "(poll every ${intervalMs / 1000}s)"
                } else {
                    "[${currentProfile.emoji} ${currentProfile.label}] health check failed"
                }
            }
        }
    }

    private fun pollHealthz(log: StringBuilder): Boolean {
        repeat(10) { attempt ->
            try {
                Thread.sleep(300)
                val url = URL("http://127.0.0.1:$bindPort/healthz")
                val conn = url.openConnection() as HttpURLConnection
                conn.connectTimeout = 1000
                conn.readTimeout = 1000
                val code = conn.responseCode
                val body = conn.inputStream.bufferedReader().readText()
                conn.disconnect()
                val maxPreview = healthBodyPreviewMaxChars(currentProfile)
                val bodyPreview = if (body.length > maxPreview) body.take(maxPreview) + "…(truncated)" else body
                log.appendLine("attempt ${attempt + 1}: GET /healthz -> $code \"$bodyPreview\"")
                trimLogBuffer(log)
                if (code == 200) return true
            } catch (e: Exception) {
                log.appendLine("attempt ${attempt + 1}: GET /healthz failed: ${e.message}")
                trimLogBuffer(log)
            }
        }
        return false
    }

    override fun onDestroy() {
        super.onDestroy()
        healthPollJob?.cancel()
        powerConnectionReceiver?.let {
            try {
                unregisterReceiver(it)
            } catch (_: IllegalArgumentException) {
                // 未登録のまま呼ばれても(onCreateの早期return等)無視する。
            }
        }
        serverProcess?.destroy()
        if (wakeLock?.isHeld == true) {
            wakeLock?.release()
        }
    }
}
