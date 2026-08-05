import Foundation

/// 電源・省電力プロファイル(Android版`PowerProfile.kt`と同じ4分類、
/// 同じ`prefValue`文字列を使う——`POST /admin/power-profile`が受け取る
/// JSON配列の値と一致させ、Windows/Linux/Android/iOS全プラットフォームで
/// 同じAPI形状を共有する)。
///
/// iOS固有の正直な注記: `alwaysOn`はAndroid版のようにWakeLockでOSの
/// スリープを止める効果を持たない——iOSアプリはバックグラウンドへ回ると
/// 通常数十秒でシステムに一時停止させられ、`alwaysOn`を選んでも
/// フォアグラウンドにある間だけ効果を持つ「省電力寄りの動作を抑制する」
/// という意味に留まる(`ServerBridge`のdocコメント参照)。
public enum PowerProfile: String, CaseIterable, Sendable {
    case powerSave = "power_save"
    case memorySaver = "memory_saver"
    case normal = "normal"
    case alwaysOn = "always_on"

    public var label: String {
        switch self {
        case .powerSave: return "Power-saving (省電力)"
        case .memorySaver: return "Memory-saver (省メモリ)"
        case .normal: return "Normal (通常)"
        case .alwaysOn: return "Always-on (常時起動、フォアグラウンド限定)"
        }
    }

    /// `healthPollIntervalMs()`(Android版)と同じ考え方——ポーリング間隔
    /// だけをプロファイルに応じて変える(iOSはOSレベルの省電力APIへの
    /// 統合〈`ProcessInfo.thermalState`等〉は今回未着手、次回課題)。
    public var healthPollInterval: TimeInterval {
        switch self {
        case .powerSave: return 300
        case .memorySaver, .normal: return 60
        case .alwaysOn: return 5
        }
    }
}
