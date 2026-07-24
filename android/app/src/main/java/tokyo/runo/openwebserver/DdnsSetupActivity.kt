package tokyo.runo.openwebserver

import android.os.Bundle
import android.text.Editable
import android.text.TextWatcher
import android.view.Gravity
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import java.io.BufferedReader
import java.io.InputStreamReader
import java.io.OutputStreamWriter
import java.net.HttpURLConnection
import java.net.URL
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * DuckDNS DDNSドメイン設定画面(2026-07-24新設)。
 *
 * Rust本体(`crates/open-web-server-gateway/src/free_domain.rs`)が既に
 * 実装している「DuckDNSトークン登録・5分間隔でのIP自動更新・最大20ドメイン
 * 管理」機能を、起動中のローカルopen-web-serverプロセスの管理API
 * (`http://127.0.0.1:<port>/admin/ddns/...`)経由でこのアプリ内から使える
 * ようにする。管理APIそのもの・自動更新ループ自体はRust側が既に持っている
 * ため、このActivityは(a)トークン入力・登録、(b)登録済み一覧のポーリング
 * 表示、(c)削除、の3操作に徹する薄いUI層。
 *
 * **セキュリティ**: 管理APIトークン・DuckDNSトークンはどちらも
 * [SecureDdnsStore]([EncryptedSharedPreferences]経由)にのみ保存し、平文の
 * `SharedPreferences`・ログには一切出力しない。
 */
class DdnsSetupActivity : AppCompatActivity() {

    private val client by lazy { DdnsApiClient(MainActivity.SERVER_PORT) }
    private var pollJob: Job? = null

    private lateinit var adminTokenInput: EditText
    private lateinit var subdomainInput: EditText
    private lateinit var duckdnsTokenInput: EditText
    private lateinit var registerButton: Button
    private lateinit var registerResultText: TextView
    private lateinit var pollStatusText: TextView
    private lateinit var domainListContainer: LinearLayout

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_ddns_setup)
        title = getString(R.string.ddns_setup_title)

        adminTokenInput = findViewById(R.id.adminTokenInput)
        subdomainInput = findViewById(R.id.subdomainInput)
        duckdnsTokenInput = findViewById(R.id.duckdnsTokenInput)
        registerButton = findViewById(R.id.registerButton)
        registerResultText = findViewById(R.id.registerResultText)
        pollStatusText = findViewById(R.id.pollStatusText)
        domainListContainer = findViewById(R.id.domainListContainer)

        // 保存済みの値をプリフィルする(トークン自体を画面に平文表示する
        // ことにはなるが、これは「マスク入力欄への復元」であり、
        // ログ出力・平文ファイル保存とは異なる——ユーザー本人が入力した
        // 値を本人が見るための入力欄。既存トークンを保持するのは
        // EncryptedSharedPreferencesのみ)。
        SecureDdnsStore.getAdminToken(this)?.let { adminTokenInput.setText(it) }
        SecureDdnsStore.getLastDuckDnsToken(this)?.let { duckdnsTokenInput.setText(it) }

        // 管理トークンは入力するたびに保存する(このActivityを閉じた後、
        // MainActivity側のサーバー起動時にも同じ値を環境変数として渡す
        // ため——値を確定操作[登録ボタン]まで待たず随時保存する設計)。
        adminTokenInput.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(s: Editable?) {
                val value = s?.toString()?.trim().orEmpty()
                if (value.isNotEmpty()) {
                    SecureDdnsStore.setAdminToken(this@DdnsSetupActivity, value)
                }
            }
        })

        registerButton.setOnClickListener { onRegisterClicked() }

        startPolling()
    }

    private fun onRegisterClicked() {
        val adminToken = adminTokenInput.text.toString().trim()
        val subdomain = subdomainInput.text.toString().trim()
        val duckdnsToken = duckdnsTokenInput.text.toString().trim()

        if (adminToken.isEmpty()) {
            registerResultText.text = "管理APIトークンを入力してください(サーバー起動時に設定した値と同じもの)。"
            return
        }
        if (subdomain.isEmpty() || duckdnsToken.isEmpty()) {
            registerResultText.text = "サブドメイン名とDuckDNSトークンの両方を入力してください。"
            return
        }

        SecureDdnsStore.setAdminToken(this, adminToken)
        SecureDdnsStore.setLastDuckDnsToken(this, duckdnsToken)

        registerButton.isEnabled = false
        registerResultText.text = "登録中..."
        CoroutineScope(Dispatchers.Main).launch {
            val result = withContext(Dispatchers.IO) {
                client.setupFreeDomain(adminToken, subdomain, duckdnsToken)
            }
            registerButton.isEnabled = true
            registerResultText.text = result
            refreshDomainList(adminToken)
        }
    }

    /**
     * 「現在のDDNS状態」の定期ポーリング(2026-07-24追加)。5分間隔の
     * サーバー側自動更新ループ自体は`free_domain.rs`が既に持っているため、
     * このActivityは表示のためだけに`GET /admin/ddns/domains`を短い間隔
     * (15秒)でポーリングする——Rust側の`DomainRegistry`が記録する
     * `last_update`(直近の更新成功/失敗・反映IP・確認時刻)をそのまま
     * 表示する。
     */
    private fun startPolling() {
        pollJob?.cancel()
        pollJob = CoroutineScope(Dispatchers.Main).launch {
            while (isActive) {
                val adminToken = adminTokenInput.text.toString().trim()
                if (adminToken.isNotEmpty()) {
                    refreshDomainList(adminToken)
                } else {
                    pollStatusText.text = "管理APIトークンを入力すると一覧を取得します。"
                }
                delay(15_000L)
            }
        }
    }

    private suspend fun refreshDomainList(adminToken: String) {
        val (statusLine, domains) = withContext(Dispatchers.IO) {
            client.listDomains(adminToken)
        }
        pollStatusText.text = statusLine
        renderDomainList(adminToken, domains)
    }

    private fun renderDomainList(adminToken: String, domains: List<DdnsApiClient.DomainEntry>) {
        domainListContainer.removeAllViews()
        if (domains.isEmpty()) {
            val empty = TextView(this)
            empty.text = "登録済みドメインはまだありません。"
            empty.textSize = 12f
            domainListContainer.addView(empty)
            return
        }
        val timeFormat = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.getDefault())
        for (entry in domains) {
            val row = LinearLayout(this)
            row.orientation = LinearLayout.HORIZONTAL
            row.gravity = Gravity.CENTER_VERTICAL
            row.setPadding(0, 12, 0, 12)

            val infoText = TextView(this)
            val statusLine = when {
                entry.lastUpdateOk == null -> "まだ更新試行なし"
                entry.lastUpdateOk -> "OK (IP: ${entry.lastUpdateIp ?: "不明"}, ${timeFormat.format(Date(entry.lastUpdateAtUnix!! * 1000))})"
                else -> "失敗 (${timeFormat.format(Date(entry.lastUpdateAtUnix!! * 1000))})"
            }
            infoText.text = "${entry.fullHostname}\n直近の更新: $statusLine"
            infoText.textSize = 12f
            val infoParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
            row.addView(infoText, infoParams)

            val deleteButton = Button(this)
            deleteButton.text = "削除"
            deleteButton.setOnClickListener {
                deleteButton.isEnabled = false
                CoroutineScope(Dispatchers.Main).launch {
                    val message = withContext(Dispatchers.IO) {
                        client.deleteDomain(adminToken, entry.domain)
                    }
                    Toast.makeText(this@DdnsSetupActivity, message, Toast.LENGTH_SHORT).show()
                    refreshDomainList(adminToken)
                }
            }
            row.addView(deleteButton)

            domainListContainer.addView(row)
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        pollJob?.cancel()
    }
}

/**
 * `/admin/ddns/...` 管理APIの薄いHTTPクライアント(`java.net.HttpURLConnection`
 * のみ、既存`MainActivity`のヘルスチェック実装と同じ作法——新規HTTP
 * ライブラリ依存を追加しない)。
 */
class DdnsApiClient(private val port: Int) {

    data class DomainEntry(
        val domain: String,
        val fullHostname: String,
        val lastUpdateOk: Boolean?,
        val lastUpdateIp: String?,
        val lastUpdateAtUnix: Long?,
    )

    private fun baseUrl() = "http://127.0.0.1:$port"

    fun setupFreeDomain(adminToken: String, domain: String, token: String): String {
        return try {
            val url = URL("${baseUrl()}/admin/ddns/setup-free-domain")
            val conn = url.openConnection() as HttpURLConnection
            conn.requestMethod = "POST"
            conn.setRequestProperty("Content-Type", "application/json")
            conn.setRequestProperty("x-admin-token", adminToken)
            conn.doOutput = true
            conn.connectTimeout = 5000
            conn.readTimeout = 5000

            val body = JSONObject().apply {
                put("domain", domain)
                put("token", token)
            }
            OutputStreamWriter(conn.outputStream).use { it.write(body.toString()) }

            val code = conn.responseCode
            val stream = if (code in 200..299) conn.inputStream else conn.errorStream
            val responseText = stream?.let { BufferedReader(InputStreamReader(it)).readText() } ?: ""
            conn.disconnect()

            if (code in 200..299) {
                val json = runCatching { JSONObject(responseText) }.getOrNull()
                json?.optString("message") ?: "登録に成功しました(status $code)。"
            } else {
                "登録に失敗しました(status $code): $responseText"
            }
        } catch (e: Exception) {
            "登録中にエラーが発生しました: ${e.message}"
        }
    }

    /** @return (状態表示用の1行サマリ, ドメイン一覧) */
    fun listDomains(adminToken: String): Pair<String, List<DomainEntry>> {
        return try {
            val url = URL("${baseUrl()}/admin/ddns/domains")
            val conn = url.openConnection() as HttpURLConnection
            conn.requestMethod = "GET"
            conn.setRequestProperty("x-admin-token", adminToken)
            conn.connectTimeout = 5000
            conn.readTimeout = 5000

            val code = conn.responseCode
            val stream = if (code in 200..299) conn.inputStream else conn.errorStream
            val responseText = stream?.let { BufferedReader(InputStreamReader(it)).readText() } ?: ""
            conn.disconnect()

            if (code !in 200..299) {
                return "一覧取得に失敗しました(status $code): $responseText" to emptyList()
            }

            val json = JSONObject(responseText)
            val count = json.optInt("count", 0)
            val capacity = json.optInt("capacity", 0)
            val remaining = json.optInt("remaining_capacity", 0)
            val array = json.optJSONArray("domains")
            val list = mutableListOf<DomainEntry>()
            if (array != null) {
                for (i in 0 until array.length()) {
                    val item = array.getJSONObject(i)
                    val lastUpdate = item.optJSONObject("last_update")
                    list.add(
                        DomainEntry(
                            domain = item.optString("domain"),
                            fullHostname = item.optString("full_hostname"),
                            lastUpdateOk = lastUpdate?.optBoolean("ok"),
                            lastUpdateIp = lastUpdate?.optString("ip")?.takeIf { it.isNotEmpty() && it != "null" },
                            lastUpdateAtUnix = lastUpdate?.optLong("checked_at_unix"),
                        )
                    )
                }
            }
            "登録済み $count/$capacity 件(残り $remaining 件)" to list
        } catch (e: Exception) {
            "一覧取得中にエラーが発生しました: ${e.message}" to emptyList()
        }
    }

    fun deleteDomain(adminToken: String, domain: String): String {
        return try {
            val url = URL("${baseUrl()}/admin/ddns/domains/$domain")
            val conn = url.openConnection() as HttpURLConnection
            conn.requestMethod = "DELETE"
            conn.setRequestProperty("x-admin-token", adminToken)
            conn.connectTimeout = 5000
            conn.readTimeout = 5000

            val code = conn.responseCode
            val stream = if (code in 200..299) conn.inputStream else conn.errorStream
            val responseText = stream?.let { BufferedReader(InputStreamReader(it)).readText() } ?: ""
            conn.disconnect()

            if (code in 200..299) "削除しました: $domain" else "削除に失敗しました(status $code): $responseText"
        } catch (e: Exception) {
            "削除中にエラーが発生しました: ${e.message}"
        }
    }
}
