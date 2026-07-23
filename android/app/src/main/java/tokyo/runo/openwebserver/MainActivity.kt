package tokyo.runo.openwebserver

import android.os.Bundle
import android.widget.Button
import android.widget.TextView
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
 * open-web-server Android版シェル(最小実装、2026-07-23着手)。
 *
 * このActivity自体はサーバー機能を一切実装しない。クロスコンパイル済みの
 * `open-web-server`ネイティブ実行ファイル(`jniLibs/arm64-v8a/libopenwebserver.so`
 * として同梱——nativeLibraryDir配下に配置することでAndroid 10+のW^X制約下でも
 * 実行可能にする、Termux等が使う既知の手法)を`ProcessBuilder`で起動し、
 * 起動後に自分自身へ`GET /healthz`を投げて実際に応答することを画面上で確認できる
 * ようにするだけの最小限のシェル。
 *
 * スコープ(意図的に今回含めない、詳細はリポジトリ`CLAUDE.md`のHANDOFF節参照):
 * フォアグラウンドサービス化、3電源プロファイルUI(省電力/常時電源接続/通常)、
 * APK署名・配布。
 */
class MainActivity : AppCompatActivity() {

    private var serverProcess: Process? = null
    private val bindPort = 18099

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        val statusText = findViewById<TextView>(R.id.statusText)
        val logText = findViewById<TextView>(R.id.logText)
        val startButton = findViewById<Button>(R.id.startButton)

        statusText.text = "open-web-server (Android shell, not started)"

        startButton.setOnClickListener {
            startButton.isEnabled = false
            CoroutineScope(Dispatchers.Main).launch {
                val log = StringBuilder()
                statusText.text = "starting..."
                val startResult = withContext(Dispatchers.IO) { startServerProcess(log) }
                if (!startResult) {
                    statusText.text = "failed to start (see log)"
                    logText.text = log.toString()
                    startButton.isEnabled = true
                    return@launch
                }

                // ネイティブプロセスがリスンし始めるまで少し待ってからヘルス
                // チェックする(即座に叩くとACCEPT前でconnection refusedになり得る)。
                val healthOk = withContext(Dispatchers.IO) { pollHealthz(log) }
                statusText.text = if (healthOk) {
                    "RUNNING: GET /healthz responded 200 from the native binary"
                } else {
                    "started, but /healthz did not respond (see log)"
                }
                logText.text = log.toString()
                startButton.isEnabled = true
            }
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
    }
}
