package tokyo.runo.openwebserver

import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.PowerManager
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import java.io.BufferedReader
import java.io.File
import java.io.InputStreamReader
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * open-web-server Android版シェル(2026-07-23着手、2026-07-24に3電源
 * プロファイル対応・open-easy-web連携導線を追加)。
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
    }

    private var serverProcess: Process? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private val bindPort = 18099

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

        val statusText = findViewById<TextView>(R.id.statusText)
        val logText = findViewById<TextView>(R.id.logText)
        val startButton = findViewById<Button>(R.id.startButton)
        val openEasyWebButton = findViewById<Button>(R.id.openEasyWebButton)
        val changeProfileButton = findViewById<Button>(R.id.changeProfileButton)

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
            }
        }

        openEasyWebButton.setOnClickListener {
            openEasyWeb()
        }

        changeProfileButton.setOnClickListener {
            startActivity(Intent(this, ProfileSelectActivity::class.java))
            finish()
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
            PowerProfile.NORMAL -> {
                log.appendLine("power: no WakeLock acquired (normal profile)")
            }
        }
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
                log.appendLine("attempt ${attempt + 1}: GET /healthz -> $code \"$body\"")
                if (code == 200) return true
            } catch (e: Exception) {
                log.appendLine("attempt ${attempt + 1}: GET /healthz failed: ${e.message}")
            }
        }
        return false
    }

    override fun onDestroy() {
        super.onDestroy()
        serverProcess?.destroy()
        if (wakeLock?.isHeld == true) {
            wakeLock?.release()
        }
    }
}
