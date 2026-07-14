//! Apache↔Tomcat型の「Webサーバー / アプリケーションサーバー」連携。
//!
//! `open-web-server-gateway` はこのモジュール無しでも完全に単体動作する
//! (課金/決済ハンドラは常に自前で処理する)。`OPEN_WEB_SERVER_APP_UPSTREAM`
//! 環境変数が設定されている場合に限り、既存ハンドラ・`tenant_router`の
//! いずれにも一致しなかったリクエストを、より高速な動的処理を担う
//! アプリケーションサーバー層(`open-runo` または `poem-cosmo-tauri` の
//! `open-runo-router`、既定では `0.0.0.0:8080` で待受)へ転送する。
//!
//! Apache が静的配信+`mod_proxy_ajp`でTomcatへ動的処理を委譲し、Tomcat
//! 単体でも直接HTTPを受けられるのと同じ関係——ここではAJPではなく単純な
//! HTTPリバースプロキシで代替する(既存の`open-easyweb`
//! `gen-vhost.sh --stack=proxy`がnginx/Apache→本ゲートウェイ間で使うのと
//! 同じ形式に揃えた)。
//!
//! **2026-07-14 統合**: 転送処理そのものは`tenant_router`経由の
//! マルチテナント転送と共有の`proxy::forward_to()`に集約した(以前は
//! ここで`hyper_util::client::legacy::Client`を毎回生成していたが、
//! `tenant_router`側で先に実装済みだったプロセス共有クライアントに
//! 揃えて重複を解消)。このモジュールは「単一アップストリームのみを
//! 環境変数から読む」責務だけを残す薄いラッパーになった。

use std::env;

const APP_UPSTREAM_ENV: &str = "OPEN_WEB_SERVER_APP_UPSTREAM";

/// アプリケーションサーバー層への転送先URL(例: `http://127.0.0.1:8080`)。
/// 環境変数が未設定なら `None`(=単体動作、このモジュールは一切使われない)。
pub fn app_upstream_base() -> Option<String> {
    env::var(APP_UPSTREAM_ENV)
        .ok()
        .map(|v| v.trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
}
