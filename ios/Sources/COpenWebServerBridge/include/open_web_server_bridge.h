// open-web-server: iOSブリッジのC ABIヘッダー。
//
// `crates/open-web-server-ios-bridge`(Rust、crate-type = cdylib/staticlib)
// が実装する関数の宣言。Swift Package Manager の `COpenWebServerBridge`
// ターゲット(module.modulemap経由)、またはXcodeプロジェクトの
// Bridging Header からこのファイルを読み込む。
//
// 実体(.a/.dylib)はこのリポジトリには含まれない——`cargo build --target
// aarch64-apple-ios` 等でmacOS環境にてビルドし、`Frameworks/`配下または
// リンカ設定でこのプロジェクトへ組み込むこと(README.md参照)。

#ifndef OPEN_WEB_SERVER_BRIDGE_H
#define OPEN_WEB_SERVER_BRIDGE_H

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// `key=value`形式の環境変数を1件設定する。`owic_start()`より前に呼ぶこと。
// 例: `OPEN_WEB_SERVER_BIND` = `127.0.0.1:18099`。
bool owic_set_env(const char *key, const char *value);

// サーバーを起動する(非ブロッキング)。2回目以降の呼び出しは何もせず
// falseを返す。
bool owic_start(void);

// 起動処理が呼ばれ済みかを返す(実際にポート受付が始まっている保証は
// しない——`GET /healthz`を別途ポーリングして確認すること)。
bool owic_is_started(void);

// グレースフルシャットダウン。現状未実装、常にfalseを返す
// (`open-web-server-ios-bridge` の lib.rs docコメント参照)。
bool owic_stop(void);

#ifdef __cplusplus
}
#endif

#endif // OPEN_WEB_SERVER_BRIDGE_H
