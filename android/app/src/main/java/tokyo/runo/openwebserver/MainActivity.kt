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
 * 連携導線を追加、2026-07-26に**排他選択→組み合わせ選択(`ActiveProfiles`)**
 * へ再設計)。
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
        /** カンマ区切りの`pref_value`一覧(空文字列=通常)。 */
        const val EXTRA_PROFILES = "profiles"

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
     * 定期ヘルスチェックのポーリング間隔(2026-07-24追加、2026-07-26に
     * 組み合わせ対応へ変更)。デスクトップ版`power_profile.rs::
     * effective_settings()`と同じ合成ルール: 常時電源接続が有効なら
     * (省電力の有無に関わらず)最優先で短縮、次に省電力単独なら延長、
     * どちらも無ければ通常間隔。省メモリはこの軸に影響しない(独立軸)。
     */
    private fun healthPollIntervalMs(profiles: ActiveProfiles): Long = when {
        profiles.alwaysOn -> 5_000L // 5秒(常時電源接続が省電力に優先する)
        profiles.powerSave -> 5 * 60_000L // 5分
        else -> 60_000L // 1分(通常・省メモリ単独)
    }

    /**
     * 省メモリの具体的施策その1(2026-07-24追加、2026-07-26に組み合わせ
     * 対応)。ログ画面(`logText`)に保持する行数の上限——省メモリが有効
     * なら(他フラグの状態に関わらず)最も厳しい上限を採用する独立軸。
     */
    private fun logBufferMaxLines(profiles: ActiveProfiles): Int = when {
        profiles.memorySaver -> 40
        profiles.alwaysOn -> 2000
        else -> 400
    }

    /**
     * 省メモリの具体的施策その2。ヘルスチェックの結果本文保持サイズ。
     * ログ行数上限と同じ合成ルール(省メモリが独立して最優先)。
     */
    private fun healthBodyPreviewMaxChars(profiles: ActiveProfiles): Int = when {
        profiles.memorySaver -> 64
        profiles.alwaysOn -> 4096
        else -> 512
    }

    /**
     * ログバッファを`logBufferMaxLines(currentProfiles)`件までに切り詰める
     * (先頭[古い行]から破棄)。
     */
    private fun trimLogBuffer(log: StringBuilder) {
        val maxLines = logBufferMaxLines(currentProfiles)
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
     * `OPEN_WEB_SERVER_ACCEL_BACKEND`)。常時電源接続が有効な場合のみ
     * 実際のハードウェア検出結果を使う(2026-07-26: 組み合わせでも
     * この判定基準は変えない、常時電源接続以外の組み合わせでは`"cpu"`)。
     */
    private fun accelBackendEnvValue(profiles: ActiveProfiles): String =
        if (profiles.alwaysOn) hardwareDetection.toAccelBackendEnvValue() else "cpu"

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

    private lateinit var currentProfiles: ActiveProfiles

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        currentProfiles = resolveProfiles()
        PowerProfileStore.save(this, currentProfiles)
        hardwareDetection = HardwareAccelDetector.detect(this)

        val statusText = findViewById<TextView>(R.id.statusText)
        val logText = findViewById<TextView>(R.id.logText)
        val startButton = findViewById<Button>(R.id.startButton)
        val openEasyWebButton = findViewById<Button>(R.id.openEasyWebButton)
        val changeProfileButton = findViewById<Button>(R.id.changeProfileButton)
        val ddnsSetupButton = findViewById<Button>(R.id.ddnsSetupButton)
        val hardwareInfoButton = findViewById<Button>(R.id.hardwareInfoButton)
        val externalStorageButton = findViewById<Button>(R.id.externalStorageButton)

        hardwareInfoButton.setOnClickListener {
            showHardwareInfoDialog()
        }

        externalStorageButton.setOnClickListener {
            showExternalStorageDialog()
        }

        statusText.text =
            "open-web-server [${profileDisplayTag(currentProfiles)}] (not started)"

        startButton.setOnClickListener {
            startButton.isEnabled = false
            CoroutineScope(Dispatchers.Main).launch {
                val log = StringBuilder()
                log.appendLine("profiles: ${currentProfiles.displayLabels().joinToString("+")} (${currentProfiles.toPrefValues().joinToString(",")})")
                statusText.text = "[${profileDisplayTag(currentProfiles)}] starting..."
                val startResult = withContext(Dispatchers.IO) { startServerProcess(log) }
                if (!startResult) {
                    statusText.text = "[${profileDisplayTag(currentProfiles)}] failed to start (see log)"
                    logText.text = log.toString()
                    startButton.isEnabled = true
                    return@launch
                }

                applyProfilePowerBehavior(log)

                // ネイティブプロセスがリスンし始めるまで少し待ってからヘルス
                // チェックする(即座に叩くとACCEPT前でconnection refusedになり得る)。
                val healthOk = withContext(Dispatchers.IO) { pollHealthz(log) }
                statusText.text = if (healthOk) {
                    "[${profileDisplayTag(currentProfiles)}] RUNNING: GET /healthz responded 200"
                } else {
                    "[${profileDisplayTag(currentProfiles)}] started, but /healthz did not respond (see log)"
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
        // 出来るようにして」、かつ「4つのモードを組み合わせで選択出来る
        // ようにして」)**: `showLiveProfileSwitchDialog()`をチェックボックス
        // による**複数選択**ダイアログへ差し替え、稼働中のプロセス・
        // Activityを維持したまま組み合わせを切り替えられるようにした
        // (再起動が必要な項目は正直にダイアログ内で案内する、下記
        // `switchProfileLive()`のdoc参照)。
        changeProfileButton.setOnClickListener {
            showLiveProfileSwitchDialog()
        }

        ddnsSetupButton.setOnClickListener {
            startActivity(Intent(this, DdnsSetupActivity::class.java))
        }

        registerPowerConnectionReceiver()
    }

    /** ステータス表示用の短いタグ(例: `🧠✕ 省メモリ+🔋⚡️✕ 省電力`)。 */
    private fun profileDisplayTag(profiles: ActiveProfiles): String {
        val flags = profiles.activeFlags()
        return if (flags.isEmpty()) {
            "${PowerProfile.NORMAL.emoji} ${PowerProfile.NORMAL.label}"
        } else {
            flags.joinToString("+") { "${it.emoji} ${it.label}" }
        }
    }

    /**
     * 電源の抜き差しを監視する(2026-07-24追加、ユーザー指示「常時電源
     * 接続版は…電源から外したら自動で、デフォルトは省電力モード、
     * もしくは通常版に切り替えますか?と質問して切り替える」)。
     *
     * - 常時電源接続が有効な状態で`ACTION_POWER_DISCONNECTED`を受信したら、
     *   「省電力に切り替えますか?それとも通常のままにしますか?」と
     *   ダイアログで質問する(既定の推奨選択肢は省電力)。
     * - 常時電源接続が無効な状態で`ACTION_POWER_CONNECTED`を受信したら、
     *   常時電源接続を有効にするかを尋ねる導線も追加。
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
     * 電源切断時の確認ダイアログ(2026-07-24、2択→3択へ変更、2026-07-26に
     * 組み合わせ選択の下でも維持——ここでの提案自体は依然として単一の
     * 具体的な組み合わせを提示する簡潔なUXとし、細かい組み合わせ調整は
     * `changeProfileButton`のチェックボックスダイアログに委ねる)。
     */
    private fun onPowerDisconnected() {
        if (!currentProfiles.alwaysOn) return
        if (isFinishing || isDestroyed) return
        AlertDialog.Builder(this)
            .setTitle("電源が外れました")
            .setMessage(
                "常時電源接続モードで動作中に電源が外れました。\n" +
                    "省電力に切り替えますか?省メモリに切り替えますか?\n" +
                    "もしくは通常のままにしますか?\n" +
                    "(推奨: 省電力)"
            )
            .setPositiveButton("省電力へ切替") { _, _ ->
                switchProfileAndRestart(ActiveProfiles(powerSave = true))
            }
            .setNeutralButton("省メモリへ切替") { _, _ ->
                switchProfileAndRestart(ActiveProfiles(memorySaver = true))
            }
            .setNegativeButton("通常のままにする") { _, _ ->
                switchProfileAndRestart(ActiveProfiles())
            }
            .setCancelable(false)
            .show()
    }

    private fun onPowerConnected() {
        if (currentProfiles.alwaysOn) return
        if (isFinishing || isDestroyed) return
        AlertDialog.Builder(this)
            .setTitle("電源が接続されました")
            .setMessage("常時電源接続(ハードウェアアクセラレーター対応)に切り替えますか?")
            .setPositiveButton("常時電源接続へ切替") { _, _ ->
                switchProfileAndRestart(ActiveProfiles(alwaysOn = true))
            }
            .setNegativeButton("このままにする", null)
            .show()
    }

    /**
     * プロファイル組み合わせを保存し、稼働中のサーバープロセスを終了・
     * `MainActivity`を再起動して新しい組み合わせで再起動させる
     * (WakeLock取得の有無・ポーリング間隔・アクセラレータ指定は
     * プロセス起動時に確定する値のため、切替には再起動が必要)。
     */
    private fun switchProfileAndRestart(newProfiles: ActiveProfiles) {
        PowerProfileStore.save(this, newProfiles)
        Toast.makeText(
            this,
            "${profileDisplayTag(newProfiles)} へ切り替えます",
            Toast.LENGTH_SHORT
        ).show()
        val intent = Intent(this, MainActivity::class.java)
        intent.putExtra(EXTRA_PROFILES, newProfiles.toPrefValues().joinToString(","))
        startActivity(intent)
        finish()
    }

    /**
     * 稼働中のサーバープロセス・Activityを終了させずにプロファイルの
     * **組み合わせ**を切り替える(2026-07-26新設・複数選択対応、
     * ユーザー指示「途中からでも選択出来るようにして」への対応、
     * `switchProfileAndRestart()`との違いはdoc参照)。押されたボタン
     * (`changeProfileButton`)から呼ばれる。
     *
     * 3つのチェックボックス(現在の状態を初期値として反映)+「適用」
     * ボタンのダイアログを表示し、決定した組み合わせを即座に
     * `switchProfileLive()`へ渡す。
     */
    private fun showLiveProfileSwitchDialog() {
        val items = arrayOf(
            "${PowerProfile.MEMORY_SAVER.emoji} ${PowerProfile.MEMORY_SAVER.label}",
            "${PowerProfile.POWER_SAVE.emoji} ${PowerProfile.POWER_SAVE.label}",
            "${PowerProfile.ALWAYS_ON.emoji} ${PowerProfile.ALWAYS_ON.label}",
        )
        val checked = booleanArrayOf(
            currentProfiles.memorySaver,
            currentProfiles.powerSave,
            currentProfiles.alwaysOn,
        )
        AlertDialog.Builder(this)
            .setTitle("電源プロファイルを切り替え(組み合わせ選択、再起動不要)")
            .setMultiChoiceItems(items, checked) { _, which, isChecked ->
                checked[which] = isChecked
            }
            .setPositiveButton("適用") { _, _ ->
                switchProfileLive(
                    ActiveProfiles(
                        memorySaver = checked[0],
                        powerSave = checked[1],
                        alwaysOn = checked[2],
                    )
                )
            }
            .setNegativeButton("キャンセル", null)
            .show()
    }

    /**
     * プロファイルの組み合わせを、アプリ・サーバープロセスを再起動せずに
     * 切り替える(2026-07-26新設)。`switchProfileAndRestart()`(電源
     * 切断/再接続ダイアログ用、既存のまま維持)とは異なり、こちらは
     * `MainActivity`を終了せず`serverProcess`も殺さない——「途中からでも
     * 選択出来るようにして」というユーザー指示の中核。
     *
     * **実際に即座に反映される項目**:
     * - `PowerProfileStore.save()`(永続化+ホーム画面アイコンの動的切替、
     *   `representativeForIcon()`参照)。
     * - `WakeLock`の取得/解放(`applyProfilePowerBehaviorLive()`、下記)。
     * - ヘルスチェックのポーリング間隔(`startPeriodicHealthPoll()`が
     *   毎イテレーション`currentProfiles`を読み直すよう改修済みのため、
     *   次の1回の待機から新間隔になる)。
     * - ログ保持行数・ヘルスチェック本文保持サイズ(`logBufferMaxLines`/
     *   `healthBodyPreviewMaxChars`は元々`currentProfiles`を直接参照する
     *   実装のため、これも改修不要で即座に反映される)。
     *
     * **正直な開示・再起動が必要なまま残る項目**: `OPEN_WEB_SERVER_
     * ACCEL_BACKEND`環境変数は`ProcessBuilder`がネイティブプロセスを
     * 起動する瞬間にしか渡せないため、常時電源接続の有無切替に伴う
     * ハードウェアアクセラレータ指定の切替自体は、ネイティブサーバー
     * プロセスの再起動(`serverProcess`の再起動、Activity自体は再起動
     * しない)が必要なまま——この制約はToastで正直に案内する。
     */
    private fun switchProfileLive(newProfiles: ActiveProfiles) {
        if (newProfiles == currentProfiles) return
        val previousProfiles = currentProfiles
        currentProfiles = newProfiles
        PowerProfileStore.save(this, newProfiles)

        val log = StringBuilder()
        applyProfilePowerBehaviorLive(previousProfiles, log)

        Toast.makeText(
            this,
            "${profileDisplayTag(newProfiles)} へ切替(再起動なし)",
            Toast.LENGTH_SHORT
        ).show()

        val statusText = findViewById<TextView>(R.id.statusText)
        if (serverProcess?.isAlive == true) {
            statusText.text = "[${profileDisplayTag(newProfiles)}] RUNNING (switched live, no restart)"
        } else {
            statusText.text =
                "open-web-server [${profileDisplayTag(newProfiles)}] (not started)"
        }

        if (log.isNotEmpty()) {
            android.util.Log.i("open-web-server", log.toString().trim())
        }

        if (accelBackendEnvValue(previousProfiles) != accelBackendEnvValue(newProfiles)
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
    private fun applyProfilePowerBehaviorLive(previousProfiles: ActiveProfiles, log: StringBuilder) {
        val needsWakeLock = currentProfiles.alwaysOn
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
                log.appendLine("power: acquired PARTIAL_WAKE_LOCK live (always-on enabled)")
            } catch (e: Exception) {
                log.appendLine("power: failed to acquire WakeLock live: ${e.message}")
            }
        } else if (!needsWakeLock && hasWakeLock) {
            wakeLock?.release()
            wakeLock = null
            log.appendLine(
                "power: released WakeLock live (always-on disabled, was: " +
                    "${previousProfiles.displayLabels().joinToString("+")})"
            )
        }
    }

    /**
     * `activity-alias`(専用ホーム画面アイコン)経由なら`Intent.action`から、
     * `ProfileSelectActivity`経由なら`EXTRA_PROFILES`(カンマ区切り)から、
     * どちらでも無い(直接`MainActivity`が再利用された等)場合は前回保存値
     * から、プロファイルの組み合わせを決定する。
     */
    private fun resolveProfiles(): ActiveProfiles {
        return when (intent?.action) {
            "tokyo.runo.openwebserver.LAUNCH_MEMORY_SAVER" -> ActiveProfiles(memorySaver = true)
            "tokyo.runo.openwebserver.LAUNCH_POWER_SAVE" -> ActiveProfiles(powerSave = true)
            "tokyo.runo.openwebserver.LAUNCH_NORMAL" -> ActiveProfiles()
            "tokyo.runo.openwebserver.LAUNCH_ALWAYS_ON" -> ActiveProfiles(alwaysOn = true)
            else -> {
                val extra = intent?.getStringExtra(EXTRA_PROFILES)
                if (extra != null) {
                    val values = extra.split(",").map { it.trim() }.filter { it.isNotEmpty() }
                    ActiveProfiles.fromPrefValues(values)
                } else {
                    PowerProfileStore.load(this)
                }
            }
        }
    }

    /**
     * プロファイル組み合わせごとの電源管理の中身そのもの。
     * - 常時電源接続が無効: `WakeLock`を一切取得しない(=Android Doze/
     *   App Standbyに逆らわない、これが「省電力対応」の実体)。
     * - 常時電源接続が有効: `PARTIAL_WAKE_LOCK`を保持し、画面消灯後も
     *   CPUをスリープさせない(充電しっぱなしのサーバー専用機を想定)。
     * - 省メモリが有効: WakeLockの有無とは独立に、ログ保持行数・
     *   ヘルスチェック本文の保持サイズを大きく絞る(別軸の合成)。
     */
    private fun applyProfilePowerBehavior(log: StringBuilder) {
        if (currentProfiles.alwaysOn) {
            try {
                val pm = getSystemService(POWER_SERVICE) as PowerManager
                val lock = pm.newWakeLock(
                    PowerManager.PARTIAL_WAKE_LOCK,
                    "OpenWebServer::AlwaysOnWakeLock"
                )
                lock.acquire()
                wakeLock = lock
                log.appendLine("power: acquired PARTIAL_WAKE_LOCK (always-on enabled)")
            } catch (e: Exception) {
                log.appendLine("power: failed to acquire WakeLock: ${e.message}")
            }
        } else {
            log.appendLine("power: no WakeLock acquired (always-on not selected)")
        }

        if (currentProfiles.powerSave) {
            log.appendLine(
                "power: polling interval extended to ${healthPollIntervalMs(currentProfiles) / 1000}s " +
                    "(power-save enabled" +
                    (if (currentProfiles.alwaysOn) ", but overridden by always-on for this axis" else "") +
                    ")"
            )
        }

        if (currentProfiles.memorySaver) {
            // 「省電力」/「常時電源接続」とは別軸: WakeLockの有無ではなく、
            // ログ保持行数(`logBufferMaxLines`)・ヘルスチェック本文の
            // 保持サイズ(`healthBodyPreviewMaxChars`)を大きく絞ることで
            // メモリ使用量そのものを減らす、というのがこのフラグの実体。
            // 常時電源接続/省電力の状態に関わらず常に適用される(独立軸)。
            log.appendLine(
                "memory: log buffer capped at ${logBufferMaxLines(currentProfiles)} lines, " +
                    "health body preview capped at ${healthBodyPreviewMaxChars(currentProfiles)} chars " +
                    "(memory-saver enabled, no background prefetch/no large caches, " +
                    "independent of power-save/always-on state)"
            )
        }

        if (currentProfiles.isNormal) {
            log.appendLine("power: normal profile (no flags active, balanced defaults)")
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
                    "\n\n常時電源接続を有効にして起動した場合、OPEN_WEB_SERVER_ACCEL_BACKEND=\"" +
                    hardwareDetection.toAccelBackendEnvValue() +
                    "\" として渡されます。" +
                    "\nWhen started with Always-On enabled, OPEN_WEB_SERVER_ACCEL_BACKEND=\"" +
                    hardwareDetection.toAccelBackendEnvValue() +
                    "\" will be passed."
            )
            .setPositiveButton("閉じる / Close", null)
            .show()
    }

    /**
     * 外付けHDD(root化端末専用)設定ダイアログ(2026-08-04新設)。
     * `ExternalStorageConfig`へ保存するのみで、稼働中プロセスへは反映
     * しない(次回サーバー起動時から有効、`startServerProcess()`参照)。
     */
    private fun showExternalStorageDialog() {
        val container = android.widget.LinearLayout(this)
        container.orientation = android.widget.LinearLayout.VERTICAL
        val pad = (16 * resources.displayMetrics.density).toInt()
        container.setPadding(pad, pad, pad, pad)

        val messageView = TextView(this)
        messageView.text = getString(R.string.external_storage_dialog_message)
        container.addView(messageView)

        val pathInput = android.widget.EditText(this)
        pathInput.hint = getString(R.string.external_storage_path_hint)
        pathInput.setText(ExternalStorageConfig.getMountPath(this) ?: "")
        container.addView(pathInput)

        val enableCheckbox = android.widget.CheckBox(this)
        enableCheckbox.text = getString(R.string.external_storage_enable_checkbox)
        enableCheckbox.isChecked = ExternalStorageConfig.isEnabled(this)
        container.addView(enableCheckbox)

        AlertDialog.Builder(this)
            .setTitle(R.string.external_storage_dialog_title)
            .setView(container)
            .setPositiveButton(R.string.external_storage_save_button) { _, _ ->
                val path = pathInput.text.toString().trim()
                if (enableCheckbox.isChecked && path.isEmpty()) {
                    Toast.makeText(this, "マウントパスを入力してください", Toast.LENGTH_LONG).show()
                    return@setPositiveButton
                }
                ExternalStorageConfig.save(this, enableCheckbox.isChecked, path)
                Toast.makeText(this, "保存しました(次回サーバー起動から反映)", Toast.LENGTH_LONG).show()
            }
            .setNegativeButton("キャンセル", null)
            .show()
    }

    /**
     * `su`(root権限昇格)へ実際に到達できるか同期的に確認する
     * (`startServerProcess()`から`Dispatchers.IO`上で呼ばれる前提、
     * UIスレッドをブロックしない)。`su -c id`の実行に成功し、
     * 終了コード0が返れば root化済みと判断する。
     */
    private fun isRootAvailable(): Boolean {
        return try {
            val process = ProcessBuilder("su", "-c", "id").start()
            val finished = process.waitFor(3, java.util.concurrent.TimeUnit.SECONDS)
            finished && process.exitValue() == 0
        } catch (e: Exception) {
            false
        }
    }

    /**
     * `su -c`へ渡すシェルコマンド文字列組み立て用の最小限のシングル
     * クォートエスケープ(POSIX shの慣用パターン: `'`を`'\''`に置換して
     * シングルクォートで囲む)。ユーザーが入力するマウントパス・
     * 管理トークンをそのままシェル文字列へ埋め込むため、コマンド
     * インジェクション対策として必須。
     */
    private fun shellQuote(value: String): String =
        "'" + value.replace("'", "'\\''") + "'"

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

            // 外付けHDD(root化端末専用)を主ストレージにする設定
            // (2026-08-04新設)。有効化されている場合はroot到達性を実際に
            // 確認し、確認できなければ**黙って内部ストレージへフォール
            // バックせず**明確に起動を中止する——「HDDに保存されている
            // つもりが実は端末内蔵ストレージだった」という誤認事故を
            // 避けるため(ExternalStorageConfigのdoc参照)。
            val useExternalStorage = ExternalStorageConfig.isEnabled(this)
            if (useExternalStorage) {
                val mountPath = ExternalStorageConfig.getMountPath(this)
                if (mountPath.isNullOrBlank()) {
                    log.appendLine("ERROR: external storage is enabled but no mount path is configured")
                    return false
                }
                log.appendLine("external storage requested: $mountPath (checking root access...)")
                if (!isRootAvailable()) {
                    log.appendLine(
                        "ERROR: root access ('su') is not available on this device — " +
                            "external HDD storage requires a rooted device (Android Scoped Storage " +
                            "blocks direct file access to USB storage otherwise). " +
                            "Falling back to internal storage was intentionally NOT done " +
                            "to avoid the app silently writing where the user doesn't expect."
                    )
                    return false
                }
                log.appendLine("root access confirmed, launching via 'su' with data dir on external storage")
            }

            val process: Process
            if (useExternalStorage) {
                val mountPath = ExternalStorageConfig.getMountPath(this)!!
                val dataDir = ExternalStorageConfig.dataDirPath(mountPath)
                val adminTokenExport = SecureDdnsStore.getAdminToken(this)?.let { token ->
                    log.appendLine("admin token: configured (value not logged)")
                    "export OPEN_WEB_SERVER_ADMIN_TOKEN=${shellQuote(token)}; "
                } ?: ""
                // `su -c`はroot権限で単一のシェルコマンド文字列を実行する
                // ため、`ProcessBuilder.environment()`では環境変数を渡せない
                // (root shellは非rootの起動元プロセス環境を継承しない前提の
                // 実装があるため)。そのため全て`export`込みの1コマンド文字列
                // として組み立てる。
                val script = buildString {
                    append("mkdir -p ${shellQuote(dataDir)}/tls-certs; ")
                    append("cd ${shellQuote(dataDir)} && ")
                    append("export OPEN_WEB_SERVER_BIND=${shellQuote("127.0.0.1:$bindPort")}; ")
                    append("export OPEN_WEB_SERVER_ACCEL_BACKEND=${shellQuote(accelBackendEnvValue(currentProfiles))}; ")
                    append("export OPEN_WEB_SERVER_WEB_VHOSTS_FILE=${shellQuote("$dataDir/web_vhosts.toml")}; ")
                    append("export OPEN_WEB_SERVER_DOMAINS_FILE=${shellQuote("$dataDir/domains.toml")}; ")
                    append("export OPEN_WEB_SERVER_REDIRECTS_FILE=${shellQuote("$dataDir/redirects.toml")}; ")
                    append("export OPEN_WEB_SERVER_TLS_CERT_DIR=${shellQuote("$dataDir/tls-certs/")}; ")
                    append("export OPEN_WEB_SERVER_KEY_STORE_PATH=${shellQuote("$dataDir/keyguardian.json")}; ")
                    append("export OPEN_WEB_SERVER_ACME_ACCOUNT_KEY_PATH=${shellQuote("$dataDir/acme-account-key.der")}; ")
                    append(adminTokenExport)
                    append("exec ${shellQuote(binaryPath.absolutePath)}")
                }
                log.appendLine("data dir on external storage: $dataDir")
                val pb = ProcessBuilder("su", "-c", script)
                pb.redirectErrorStream(true)
                process = pb.start()
            } else {
                val pb = ProcessBuilder(binaryPath.absolutePath)
                pb.directory(filesDir)
                pb.environment()["OPEN_WEB_SERVER_BIND"] = "127.0.0.1:$bindPort"
                pb.environment()["OPEN_WEB_SERVER_ACCEL_BACKEND"] = accelBackendEnvValue(currentProfiles)
                log.appendLine("accel backend requested: ${accelBackendEnvValue(currentProfiles)}")

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
                process = pb.start()
            }
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
     * 起動後の継続的な死活監視(2026-07-24追加)。プロファイルの組み合わせ
     * ごとに間隔を変える(`healthPollIntervalMs`)ことが「省電力が実際に
     * 省電力になる」施策そのもの——省電力有効時はこのループの頻度自体を
     * 大きく落とし、CPU/無線を起こす回数を最小化する。常時電源接続が
     * 有効なら短い間隔で即応性を優先する(省電力に優先、上記doc参照)。
     */
    private fun startPeriodicHealthPoll(statusText: TextView) {
        healthPollJob?.cancel()
        healthPollJob = CoroutineScope(Dispatchers.Main).launch {
            while (isActive) {
                // **2026-07-26変更(ユーザー指示「途中からでもプロファイル
                // 切替できるように」対応)**: 毎イテレーションで
                // `currentProfiles`を読み直す形のため、アプリ再起動は
                // もちろんサーバープロセス再起動も無しに、次の1回の待機
                // から新しい間隔が反映される。
                val intervalMs = healthPollIntervalMs(currentProfiles)
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
                    "[${profileDisplayTag(currentProfiles)}] RUNNING " +
                        "(poll every ${intervalMs / 1000}s)"
                } else {
                    "[${profileDisplayTag(currentProfiles)}] health check failed"
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
                val maxPreview = healthBodyPreviewMaxChars(currentProfiles)
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
