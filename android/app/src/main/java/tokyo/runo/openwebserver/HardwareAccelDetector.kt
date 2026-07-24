package tokyo.runo.openwebserver

import android.content.Context
import android.content.pm.PackageManager
import android.hardware.display.DisplayManager
import android.opengl.EGL14
import android.opengl.EGLConfig
import android.opengl.EGLDisplay
import android.opengl.EGLSurface
import android.opengl.GLES10
import android.os.Build

/**
 * 常時電源接続版向けのハードウェア検出(2026-07-24新設、ユーザー指示
 * 「スマホにも、タブレットにも、PCにも、内部グラフィックボードにも
 * 内部GPUチップにも、外付けGPU検出にも、NPUにも対応して」を受けて、
 * 前回セッションの「外付けGPU検出は実装しない」判断を撤回し実装する)。
 *
 * **設計方針(過剰実装を避け、検出できた情報を正直に伝える)**:
 * - **内部GPU**: `EGL14`/`GLES10`で一時的なpbufferサーフェスを作り
 *   `GL_RENDERER`/`GL_VENDOR`文字列を取得する。既存のAndroidアプリ開発で
 *   一般的な最小限のEGLコンテキスト管理(生成後すぐ破棄)に留め、
 *   `GLSurfaceView`のような画面表示用の複雑なライフサイクル管理は行わない。
 * - **NPU**: NPU専用のfeature flagは標準に無いため、(a)
 *   `Build.VERSION.SDK_INT >= 27`(NNAPI導入バージョン、Android 8.1)で
 *   NNAPIが利用可能かの簡易フラグとし、(b) `Build.SOC_MODEL`/
 *   `SOC_MANUFACTURER`(Android 12+で利用可能)が取得できればSoC名も
 *   併記する。「NPU専用ハードウェアを直接検出した」とは主張しない
 *   ——正直には「NNAPI利用可能性」の判定に留まる。
 * - **外付けGPU**: Android標準には専用APIが無いため検出しない。代わりに
 *   `DisplayManager#getDisplays()`で複数ディスプレイの有無を見て、
 *   「外部ディスプレイ接続を検出した」という正直な粒度の
 *   `external_display_hint`フラグのみを立てる(「外付けGPUを検出した」
 *   という誇張はしない)。
 */
object HardwareAccelDetector {

    /**
     * 検出結果(UI表示・`OPEN_WEB_SERVER_ACCEL_BACKEND`環境変数生成の両方に使う)。
     */
    data class DetectionResult(
        val glRenderer: String?,
        val glVendor: String?,
        val vulkanSupported: Boolean,
        val nnapiLikelyAvailable: Boolean,
        val socModel: String?,
        val socManufacturer: String?,
        val externalDisplayHint: Boolean,
        val displayCount: Int
    ) {
        /**
         * `OPEN_WEB_SERVER_ACCEL_BACKEND`環境変数へ渡す文字列を生成する。
         * 例: `"gpu:Adreno 730;npu:nnapi_available;external_display_hint"`。
         * 検出できた情報のみを正直に列挙し、検出できなかった項目は含めない
         * (「検出できていないのにcpu以外を騙る」ことを避ける)。
         */
        fun toAccelBackendEnvValue(): String {
            val parts = mutableListOf<String>()

            if (!glRenderer.isNullOrBlank()) {
                parts.add("gpu:$glRenderer")
            } else if (vulkanSupported) {
                // GL_RENDERER文字列が取れなくても、Vulkan対応自体は
                // ActivityManagerの軽い判定で分かるため、その情報だけは残す。
                parts.add("gpu:vulkan_capable")
            }

            if (nnapiLikelyAvailable) {
                val socInfo = listOfNotNull(socManufacturer, socModel)
                    .joinToString(" ")
                    .ifBlank { null }
                parts.add(if (socInfo != null) "npu:nnapi_available($socInfo)" else "npu:nnapi_available")
            }

            if (externalDisplayHint) {
                parts.add("external_display_hint")
            }

            return if (parts.isEmpty()) "cpu" else parts.joinToString(";")
        }

        /**
         * 設定画面/検出画面での人間向け表示用テキスト(2026-07-24追加指示、
         * 日本語と英語を併記する)。
         */
        fun toHumanReadableSummary(): String {
            val sb = StringBuilder()

            val gpuLine = glRenderer ?: "(取得できませんでした / not available)"
            sb.appendLine("検出されたGPU: $gpuLine / Detected GPU: $gpuLine")
            if (!glVendor.isNullOrBlank()) {
                sb.appendLine("GPUベンダー: $glVendor / GPU vendor: $glVendor")
            }
            val vulkanJa = if (vulkanSupported) "あり" else "なし/不明"
            val vulkanEn = if (vulkanSupported) "yes" else "no/unknown"
            sb.appendLine("Vulkan対応: $vulkanJa / Vulkan support: $vulkanEn")

            val nnapiJa = if (nnapiLikelyAvailable) "利用可能(SDK ${Build.VERSION.SDK_INT} >= 27)" else "利用不可"
            val nnapiEn = if (nnapiLikelyAvailable) "available (SDK ${Build.VERSION.SDK_INT} >= 27)" else "not available"
            sb.appendLine("NPU(NNAPI): $nnapiJa / NPU (NNAPI): $nnapiEn")

            if (!socModel.isNullOrBlank() || !socManufacturer.isNullOrBlank()) {
                val socInfo = listOfNotNull(socManufacturer, socModel).joinToString(" ")
                sb.appendLine("SoC: $socInfo / SoC: $socInfo")
            }

            sb.appendLine("接続ディスプレイ数: $displayCount / Connected display count: $displayCount")

            val extDisplayJa = if (externalDisplayHint) {
                "検出(外付けGPUそのものを検出したわけではなく、外部ディスプレイ経由の可能性を示すヒントです)"
            } else {
                "なし"
            }
            val extDisplayEn = if (externalDisplayHint) {
                "detected (this is a hint of a possible external GPU via an external display, not a direct detection of an external GPU itself)"
            } else {
                "none"
            }
            sb.appendLine("外部ディスプレイ接続: $extDisplayJa / External display connected: $extDisplayEn")

            return sb.toString().trimEnd()
        }
    }

    fun detect(context: Context): DetectionResult {
        val gl = detectGlInfo()
        val vulkan = detectVulkanSupport(context)
        val nnapi = Build.VERSION.SDK_INT >= 27
        val (socManufacturer, socModel) = detectSoc()
        val (externalDisplayHint, displayCount) = detectExternalDisplayHint(context)

        return DetectionResult(
            glRenderer = gl?.first,
            glVendor = gl?.second,
            vulkanSupported = vulkan,
            nnapiLikelyAvailable = nnapi,
            socModel = socModel,
            socManufacturer = socManufacturer,
            externalDisplayHint = externalDisplayHint,
            displayCount = displayCount
        )
    }

    /**
     * 一時的なEGL pbufferサーフェス(画面に表示しない、1x1の最小オフスクリーン
     * バッファ)を作り`GL_RENDERER`/`GL_VENDOR`を取得したら即座に破棄する。
     * 失敗時(古い端末・ドライバの問題等)はnullを返し、呼び出し側は
     * Vulkan判定へフォールバックする。
     */
    private fun detectGlInfo(): Pair<String, String>? {
        var display: EGLDisplay? = null
        var surface: EGLSurface? = null
        var context: android.opengl.EGLContext? = null
        return try {
            display = EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY)
            if (display == EGL14.EGL_NO_DISPLAY) return null

            val version = IntArray(2)
            if (!EGL14.eglInitialize(display, version, 0, version, 1)) return null

            val configAttribs = intArrayOf(
                EGL14.EGL_RENDERABLE_TYPE, EGL14.EGL_OPENGL_ES2_BIT,
                EGL14.EGL_SURFACE_TYPE, EGL14.EGL_PBUFFER_BIT,
                EGL14.EGL_RED_SIZE, 8,
                EGL14.EGL_GREEN_SIZE, 8,
                EGL14.EGL_BLUE_SIZE, 8,
                EGL14.EGL_NONE
            )
            val configs = arrayOfNulls<EGLConfig>(1)
            val numConfigs = IntArray(1)
            if (!EGL14.eglChooseConfig(display, configAttribs, 0, configs, 0, 1, numConfigs, 0) ||
                numConfigs[0] == 0
            ) {
                return null
            }
            val config = configs[0] ?: return null

            val contextAttribs = intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION, 2, EGL14.EGL_NONE)
            context = EGL14.eglCreateContext(display, config, EGL14.EGL_NO_CONTEXT, contextAttribs, 0)
            if (context == null || context == EGL14.EGL_NO_CONTEXT) return null

            val surfaceAttribs = intArrayOf(EGL14.EGL_WIDTH, 1, EGL14.EGL_HEIGHT, 1, EGL14.EGL_NONE)
            surface = EGL14.eglCreatePbufferSurface(display, config, surfaceAttribs, 0)
            if (surface == null || surface == EGL14.EGL_NO_SURFACE) return null

            if (!EGL14.eglMakeCurrent(display, surface, surface, context)) return null

            val renderer = GLES10.glGetString(GLES10.GL_RENDERER)
            val vendor = GLES10.glGetString(GLES10.GL_VENDOR)
            if (renderer.isNullOrBlank()) null else Pair(renderer, vendor ?: "")
        } catch (_: Exception) {
            null
        } finally {
            try {
                if (display != null && display != EGL14.EGL_NO_DISPLAY) {
                    EGL14.eglMakeCurrent(
                        display, EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_CONTEXT
                    )
                    if (surface != null) EGL14.eglDestroySurface(display, surface)
                    if (context != null) EGL14.eglDestroyContext(display, context)
                    EGL14.eglTerminate(display)
                }
            } catch (_: Exception) {
                // 破棄時の例外は無視(検出結果には影響しない)。
            }
        }
    }

    /**
     * `ActivityManager#deviceHasVulkanSupport()`は実在しないAPIだった
     * (ビルドで発覚・修正)ため、標準の`PackageManager.FEATURE_VULKAN_
     * HARDWARE_VERSION`ハードウェアfeatureフラグでの軽い判定に変更。
     */
    private fun detectVulkanSupport(context: Context): Boolean {
        return try {
            context.packageManager.hasSystemFeature(PackageManager.FEATURE_VULKAN_HARDWARE_VERSION)
        } catch (_: Exception) {
            false
        }
    }

    /** Android 12+(SDK 31+)のみ`Build.SOC_MODEL`/`SOC_MANUFACTURER`が利用可能。 */
    private fun detectSoc(): Pair<String?, String?> {
        return try {
            if (Build.VERSION.SDK_INT >= 31) {
                Pair(Build.SOC_MANUFACTURER, Build.SOC_MODEL)
            } else {
                Pair(null, null)
            }
        } catch (_: Exception) {
            Pair(null, null)
        }
    }

    /**
     * 複数ディスプレイ(USB-C外部ディスプレイ・DeX的な外部出力等)の
     * 有無を見て、「外部ディスプレイ接続を検出した」という正直な粒度の
     * ヒントを返す。1件(端末本体の内蔵ディスプレイ)のみなら`false`。
     */
    private fun detectExternalDisplayHint(context: Context): Pair<Boolean, Int> {
        return try {
            val dm = context.getSystemService(Context.DISPLAY_SERVICE) as? DisplayManager
            val count = dm?.displays?.size ?: 1
            Pair(count > 1, count)
        } catch (_: Exception) {
            Pair(false, 1)
        }
    }
}
