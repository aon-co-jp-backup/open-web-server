//! iOS向けC ABIブリッジ。
//!
//! Swift側(`ios/Sources/OpenWebServerKit/ServerBridge.swift`)は、この
//! crateが公開する`extern "C"`関数を、ブリッジングヘッダー
//! (`ios/Sources/COpenWebServerBridge/include/open_web_server_bridge.h`)
//! 経由で呼び出す。
//!
//! # 正直な開示・iOSの制約(2026-08-05新設)
//!
//! - **バックグラウンド常時稼働は原則できない**: iOSはアプリがバック
//!   グラウンドへ回ると通常は数十秒でプロセスを一時停止する(Android版の
//!   ような「充電中は常時電源接続プロファイルでフォアグラウンドサービス
//!   同然に稼働し続ける」という前提は成り立たない)。`BGProcessingTask`/
//!   `NEAppPushProvider`等のiOS向けバックグラウンド実行APIとの統合は
//!   このブリッジのスコープ外(次回検討)。**このアプリがフォアグラウンド
//!   にある間だけ、他端末からHTTPで到達できる**、という制約付きの
//!   「簡易サーバー」であることを利用者に明示する必要がある。
//! - **`run()`はプロセス内で一度しか呼べない**(TCPポートbind・
//!   グローバルなtracingサブスクライバ初期化を伴うため)。`owic_start()`
//!   の2回目以降の呼び出しは何もせず`false`を返す。
//! - **正常終了(グレースフルシャットダウン)は未実装**:
//!   `open_web_server_gateway::run()`は`ctrl_c()`シグナル待ちを内部に
//!   持つが、アプリプロセス内から安全に割り込む経路(shutdown channel等)
//!   は用意されていない。`owic_stop()`は将来の拡張点としてシグネチャの
//!   みここに用意し、現状は常に`false`(未対応)を返す——「動くふりをして
//!   実際には何もしない」関数を偽って完成扱いにしないための措置。

use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static STARTED: AtomicBool = AtomicBool::new(false);
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// `key=value`形式の環境変数を1件設定する(Swift側が`owic_start()`より
/// 前に、`OPEN_WEB_SERVER_BIND`等を渡すために呼ぶ)。既存のAndroid版が
/// `ProcessBuilder`にわたす環境変数〈`OPEN_WEB_SERVER_BIND`・
/// `OPEN_WEB_SERVER_WEB_VHOSTS_FILE`等〉と同じ変数名をそのまま使える。
///
/// # Safety
/// `key`/`value`はいずれもNUL終端の有効なUTF-8 C 文字列へのポインタで
/// あること(Swift側の`String.withCString`が渡す形と一致)。
#[no_mangle]
pub unsafe extern "C" fn owic_set_env(key: *const c_char, value: *const c_char) -> bool {
    if key.is_null() || value.is_null() {
        return false;
    }
    let (Ok(key), Ok(value)) = (
        CStr::from_ptr(key).to_str(),
        CStr::from_ptr(value).to_str(),
    ) else {
        return false;
    };
    std::env::set_var(key, value);
    true
}

/// サーバーを起動する(非ブロッキング、専用スレッド上の`tokio`
/// ランタイムで`open_web_server_gateway::run()`を実行する)。
///
/// 呼び出し前に`owic_set_env()`で必要な`OPEN_WEB_SERVER_*`環境変数
/// (特に`OPEN_WEB_SERVER_BIND`、iOSでは通常`127.0.0.1:<任意ポート>`か
/// `0.0.0.0:<任意ポート>`)を設定しておくこと。
///
/// 戻り値: 実際に起動処理を開始できれば`true`、既に起動済み(2回目以降の
/// 呼び出し)なら何もせず`false`。
#[no_mangle]
pub extern "C" fn owic_start() -> bool {
    if STARTED.swap(true, Ordering::SeqCst) {
        return false;
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => {
            STARTED.store(false, Ordering::SeqCst);
            return false;
        }
    };

    // `run()`はサーバーが動き続ける限り解決しないFutureのため、専用の
    // OSスレッド上で実行する(呼び出し元のSwiftメインスレッドを塞がない)。
    std::thread::spawn(move || {
        let runtime = RUNTIME.get_or_init(|| runtime);
        runtime.block_on(async {
            if let Err(e) = open_web_server_gateway::run().await {
                eprintln!("open-web-server exited with error: {e:#}");
            }
        });
    });

    true
}

/// 起動処理が(一度でも)呼ばれ、まだ「起動済み」の状態であるかを返す。
/// **実際にリッスンソケットの受付が始まっているかまでは保証しない**——
/// `owic_start()`呼び出し直後は、TCP bind等が完了する前でも`true`を
/// 返しうる(正直な開示、`GET /healthz`をポーリングして実際の到達性を
/// 確認するのはSwift側の責務、Android版`MainActivity.pollHealthz()`と
/// 同じパターン)。
#[no_mangle]
pub extern "C" fn owic_is_started() -> bool {
    STARTED.load(Ordering::SeqCst)
}

/// グレースフルシャットダウン(未実装、モジュールdoc参照)。常に`false`。
#[no_mangle]
pub extern "C" fn owic_stop() -> bool {
    false
}
