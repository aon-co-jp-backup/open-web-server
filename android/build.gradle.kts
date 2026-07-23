// open-web-server Android shell: single-Activity Kotlin app that launches the
// cross-compiled `open-web-server` native binary via ProcessBuilder.
//
// スコープ(実装方針、詳細は`../CLAUDE.md`のHANDOFF節参照):
// 3電源プロファイルUI・フォアグラウンドサービス化・署名/配布は今回のスコープ外。
// 「実機/エミュレータ上でバイナリが実際に起動し、HTTPリクエストに応答する」
// という最小限の一気通貫の実証を最優先ゴールとする。
plugins {
    id("com.android.application") version "8.7.2" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
}
