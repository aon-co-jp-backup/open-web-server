// swift-tools-version:5.9
// open-web-server: iOS版Swiftシェル(Swift Package Manager構成)。
//
// 正直な開示: このパッケージは Rust 側の静的ライブラリ
// (`libopen_web_server_ios_bridge.a`、`aarch64-apple-ios`/
// `aarch64-apple-ios-sim` 向け)が `Libraries/` 配下に実際に配置されて
// いることを前提とする。この開発環境(Windows、macOS/Xcode不在)では
// 当該クロスビルド自体を実行できないため、`.a` ファイルは同梱していない
// ——`swift build`/Xcodeでのビルドは、README.md の手順に従って
// macOS環境で `.a` を生成し配置するまでは失敗する(これは未検証である
// ことを隠さないための意図的な構成)。

import PackageDescription

let package = Package(
    name: "OpenWebServerKit",
    platforms: [.iOS(.v16)],
    products: [
        .library(name: "OpenWebServerKit", targets: ["OpenWebServerKit"]),
    ],
    targets: [
        // Rust側 `crates/open-web-server-ios-bridge` の C ABI ヘッダー。
        .target(
            name: "COpenWebServerBridge",
            path: "Sources/COpenWebServerBridge"
        ),
        // Swiftから見た使いやすいラッパー(PowerProfile・ServerBridge等)。
        .target(
            name: "OpenWebServerKit",
            dependencies: ["COpenWebServerBridge"],
            path: "Sources/OpenWebServerKit",
            linkerSettings: [
                // README.mdの手順で`Libraries/`へ配置した静的ライブラリを
                // リンクする(macOS環境で`cargo build --target
                // aarch64-apple-ios`等を実行して生成)。
                .unsafeFlags([
                    "-L", "Libraries",
                    "-lopen_web_server_ios_bridge",
                ])
            ]
        ),
    ]
)
