import OpenWebServerKit
import SwiftUI

/// アプリのエントリポイント(SwiftUI)。
///
/// この`App/`フォルダはSwift Packageの外に置いてある——iOSの実行可能な
/// `.app`バンドル(アイコン・Info.plist・コード署名等を持つ)を作るには
/// 通常のSwift Packageだけでは完結せず、Xcodeの「iOS App」テンプレートで
/// 新規プロジェクトを作り、`../`(このディレクトリの親、`ios/`)を
/// ローカルSwift Package依存として追加した上で、この2ファイル
/// (`OpenWebServerApp.swift`・`ContentView.swift`)をXcodeプロジェクトへ
/// 追加コピーする、という手順を想定している(詳細は`ios/README.md`)。
@main
struct OpenWebServerApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}
