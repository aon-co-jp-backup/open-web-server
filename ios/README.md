# open-web-server iOS版(ソース一式、2026-08-05新設)

## 正直な開示(まず最初に)

このディレクトリのソース一式は、**この開発環境(Windows、Xcode/macOS不在)
では一度もビルド・実機/シミュレータ検証を行っていない**。Android版
(`android/`)のように「実エミュレータでの`/healthz`応答確認まで実証済み」
という段階には至っておらず、macOS環境での実際のビルド作業からが次の
ステップとなる。以下の手順・コード自体はレビューベースで作成しており、
`cargo build --target aarch64-apple-ios`のようなクロスビルドコマンド自体の
実行結果もこの環境では確認できていない。

## なぜAndroid版と設計が違うのか

Android版(`android/`)は`open-web-server`実行ファイルそのものを
`cargo ndk`でクロスビルドし、`libopenwebserver.so`という名前でAPKへ同梱、
`ProcessBuilder`でサブプロセスとして起動する設計だった。

**iOSはこの方式を許可しない**——アプリバンドル内で実行できるのは
コード署名済みのアプリ本体・App Extensionのみで、同梱した任意の実行
ファイルをサブプロセスとして起動することはできない。そのため今回は
`open-web-server-gateway`のサーバー起動ロジックを**ライブラリ関数**
(`open_web_server_gateway::run()`、2026-08-05にバイナリ側`main.rs`から
`lib.rs`へ抽出済み)として、iOSアプリのプロセス内へ直接リンクする設計と
した。

```
Swift (ContentView / ServerBridge)
  → C ABI (owic_start / owic_set_env / owic_is_started / owic_stop)
    → crates/open-web-server-ios-bridge (Rust cdylib/staticlib)
      → open-web-server-gateway::run() (通常のWindows/Linux/Androidバイナリと同じロジック)
```

## ビルド手順(macOS環境が必要、未検証)

1. Rustのiosターゲットを追加:
   ```bash
   rustup target add aarch64-apple-ios aarch64-apple-ios-sim
   ```
2. 静的ライブラリをビルド:
   ```bash
   cd crates/open-web-server-ios-bridge
   cargo build --release --target aarch64-apple-ios       # 実機
   cargo build --release --target aarch64-apple-ios-sim   # Appleシリコンシミュレータ
   ```
3. `ios/Libraries/`ディレクトリを作り、実機/シミュレータ用途に応じて
   `libopen_web_server_ios_bridge.a`をコピーする(実機・シミュレータ両対応の
   XCFrameworkとしてまとめる場合は`xcodebuild -create-xcframework`を使うこと
   ——今回は`Package.swift`側で単純な`-L`/`-l`リンクにとどめており、
   XCFramework化は次回の改善候補)。
4. Xcodeで新規「iOS App」プロジェクトを作成し、`ios/`ディレクトリを
   ローカルSwift Package依存として追加、`ios/App/`配下の2ファイル
   (`OpenWebServerApp.swift`・`ContentView.swift`)をプロジェクトへ
   追加する。

## iOSの制約(正直な開示、`ServerBridge.swift`のdocコメントにも記載)

- **バックグラウンド常時稼働は原則できない**——iOSはバックグラウンドへ
  回ったアプリを通常数十秒で一時停止させる。このアプリはフォアグラウンド
  にある間だけ、他端末からHTTPで到達できる「簡易サーバー」である。
  `BGProcessingTask`/`NEAppPushProvider`等との統合は次回課題。
- **グレースフルシャットダウンは未実装**(`owic_stop()`は常に`false`を
  返す)——起動後はアプリ終了まで動き続ける設計。
- 電源プロファイル(`PowerProfile.swift`)はAndroid版と同じ4分類・同じ
  文字列(`power_save`/`memory_saver`/`normal`/`always_on`)を持つが、iOS版は
  ヘルスチェックのポーリング間隔にのみ影響する(Android版のWakeLock相当の
  OSレベル制御は実装していない)。

## 次にすべきこと

1. macOS環境での実際のクロスビルド・XCFramework化。
2. Xcodeプロジェクトの実際の作成・実機/シミュレータでの`/healthz`応答確認
   (Android版で行った実証と同水準の検証)。
3. バックグラウンド実行(`BGProcessingTask`等)との統合検討。
4. `owic_stop()`の実装(`open_web_server_gateway::run()`側にshutdown
   channelを追加する必要がある)。
5. `open-easy-web`側にも同様のiOSブリッジを展開するかどうかの検討
   (今回は`open-web-server`側のみ着手)。
