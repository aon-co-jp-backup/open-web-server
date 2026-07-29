package tokyo.runo.openwebserver

import android.content.Intent
import android.os.Bundle
import android.widget.Button
import android.widget.CheckBox
import androidx.appcompat.app.AppCompatActivity

/**
 * 起動時の電源プロファイル選択画面(2026-07-24新設、LAUNCHER。同日中に
 * 3択→4択[省メモリ版を追加]へ拡張、2026-07-26に**排他選択(ラジオ的挙動の
 * ボタン)→組み合わせ選択(チェックボックス)**へ変更)。
 *
 * ユーザー指示「4つのモードを組み合わせで選択出来るようにして」への
 * 対応。旧実装はボタン押下=即座にその1プロファイルのみで起動、だった。
 * 新実装は[CheckBox]×3(省メモリ/省電力/常時電源接続、「通常」は独立
 * トグルではない——`ActiveProfiles`のdoc参照)+「この組み合わせで起動」
 * ボタンの構成。**互いに素なチェックボックスではない**ため、たとえば
 * 省メモリ+省電力の両方にチェックを入れて起動できる。
 */
class ProfileSelectActivity : AppCompatActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_profile_select)

        val checkMemorySaver = findViewById<CheckBox>(R.id.checkMemorySaver)
        val checkPowerSave = findViewById<CheckBox>(R.id.checkPowerSave)
        val checkAlwaysOn = findViewById<CheckBox>(R.id.checkAlwaysOn)
        val launchButton = findViewById<Button>(R.id.buttonLaunchCombination)

        launchButton.setOnClickListener {
            val profiles = ActiveProfiles(
                memorySaver = checkMemorySaver.isChecked,
                powerSave = checkPowerSave.isChecked,
                alwaysOn = checkAlwaysOn.isChecked,
            )
            launchWithProfiles(profiles)
        }
    }

    private fun launchWithProfiles(profiles: ActiveProfiles) {
        PowerProfileStore.save(this, profiles)
        val intent = Intent(this, MainActivity::class.java)
        intent.putExtra(MainActivity.EXTRA_PROFILES, profiles.toPrefValues().joinToString(","))
        startActivity(intent)
        finish()
    }
}
