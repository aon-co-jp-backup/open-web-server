package tokyo.runo.openwebserver

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.util.AttributeSet
import android.view.View

/**
 * 使用率(0.0〜1.0)を「使用中/空き」の2色の円グラフとして描画する
 * 汎用カスタムView(2026-08-06新設、`MemoryInfoButton`/`DiskInfoButton`
 * 両方から共通利用する)。
 *
 * 外部グラフライブラリには依存せず、`android.graphics.Canvas`の
 * `drawArc`のみで描画する(build.gradle.ktsへの新規依存追加は無し、
 * ユーザー指示通り)。メモリ用途(実メモリ/仮想メモリ/合計の3種類)・
 * ディスク用途のどちらでも、`setUsage()`を呼ぶだけで再利用できる
 * ように、用途固有の知識(バイト数のフォーマット等)は一切持たせない
 * 設計にした。
 */
class PieChartView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null
) : View(context, attrs) {

    /** 使用中(usedRatio分)の扇形の色。既定は目立つ赤系。 */
    var usedColor: Int = Color.parseColor("#E53935")
        set(value) {
            field = value
            invalidate()
        }

    /** 空き(1-usedRatio分)の扇形の色。既定は控えめな灰緑系。 */
    var freeColor: Int = Color.parseColor("#B0BEC5")
        set(value) {
            field = value
            invalidate()
        }

    private var usedRatio: Float = 0f

    private val arcPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.FILL
    }

    private val rect = RectF()

    /**
     * 使用率を設定する。0.0未満/1.0超は安全にクランプする(呼び出し側の
     * 計算誤差[例: swap無し端末で0除算になりうる箇所]でクラッシュしない
     * ようにするための防御)。
     */
    fun setUsage(ratio: Float) {
        usedRatio = ratio.coerceIn(0f, 1f)
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)

        val size = minOf(width, height).toFloat()
        if (size <= 0f) return

        val padding = size * 0.05f
        rect.set(padding, padding, size - padding, size - padding)

        val usedSweep = usedRatio * 360f
        val freeSweep = 360f - usedSweep

        // まず空き分を12時位置から時計回りに全体描画し、その後
        // 使用中分を同じ開始位置から重ね描きする(隙間が出ない単純な
        // 2色円グラフの標準的な描き方)。
        arcPaint.color = freeColor
        canvas.drawArc(rect, -90f, freeSweep, true, arcPaint)

        arcPaint.color = usedColor
        canvas.drawArc(rect, -90f + freeSweep, usedSweep, true, arcPaint)
    }
}
