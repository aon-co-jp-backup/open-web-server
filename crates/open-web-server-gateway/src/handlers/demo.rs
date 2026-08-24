//! `GET /demo` — open-web-server自身の「本番/デモ分離」パターン対応
//! (2026-08-24新設、ユーザー指示)。
//!
//! **背景**: `open-easy-web`等の姉妹アプリは、共有バックエンドへの動的
//! テナント登録(「分身の術」)で`/demo`相当のデモ環境を公開しているが、
//! `open-web-server`自身にはこのパターンが未実装だった。このリポジトリは
//! Webサーバー/リバースプロキシ本体そのものであり、管理画面(WASM UI)を
//! 持たないバックエンド専用構成のため、姉妹アプリと同じ「別テナントとして
//! 同一バイナリを登録する」形の`/demo`は意味を成さない——代わりに、
//! **この製品が実際に提供する機能(テナントルーティング・vhost配信・
//! ヘルスチェック監視)を、読み取り専用で確認できる最小限のデモページ**を
//! `/demo`パスに追加する。
//!
//! **安全設計(最重要)**: このハンドラは`GET`専用であり、`x-admin-token`/
//! `KeyGuardian`のような管理認証を一切要求しない代わりに、`POST`/`DELETE`/
//! `PUT`のような破壊的操作(テナント追加・削除・vhost変更等)への導線は
//! 一切含まない——表示専用の`state.tenants.list()`/`state.web_vhosts.list()`
//! /`state.watchdog.snapshot()`の読み取りのみを行う。加えて、`backend_addr`
//! (内部ネットワークのリバースプロキシ転送先)・`db_uri`(DB接続文字列)・
//! `docroot`(サーバー上のファイルパス)といった内部トポロジ/認証情報に
//! 相当するフィールドは、たとえ読み取り専用であっても公開ページには一切
//! 表示しない(ホスト名・バックエンド種別・稼働状況の要約に限定する)。

use std::sync::Arc;

use hyper::{Response, StatusCode};

use crate::response::{html_response, BoxBody};
use crate::state::AppState;

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `GET /demo` — 現在登録されているテナント/vhostの読み取り専用一覧と、
/// 死活監視の要約を表示する紹介ページ。認証不要・破壊的操作への導線なし。
pub async fn render(state: Arc<AppState>) -> Response<BoxBody> {
    let tenants = state.tenants.list().await;
    let vhosts = state.web_vhosts.list().await;
    let watchdog_snapshot = state.watchdog.snapshot().await;

    let tenant_rows: String = if tenants.is_empty() {
        "<tr><td colspan=\"2\" class=\"empty\">(現在登録されているテナントはありません / no tenants registered yet)</td></tr>".to_string()
    } else {
        tenants
            .iter()
            .map(|t| {
                format!(
                    "<tr><td>{}</td><td>{:?}</td></tr>",
                    escape_html(&t.host),
                    t.backend
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let vhost_rows: String = if vhosts.is_empty() {
        "<tr><td class=\"empty\">(現在登録されているvhostはありません / no static/PHP vhosts registered yet)</td></tr>".to_string()
    } else {
        vhosts
            .iter()
            .map(|v| format!("<tr><td>{}</td></tr>", escape_html(&v.host)))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let total_watched = watchdog_snapshot.len();
    let healthy_count = watchdog_snapshot.iter().filter(|(_, h)| h.last_ok).count();
    let watchdog_summary = if total_watched == 0 {
        "死活監視は未設定です(`OPEN_WEB_SERVER_WATCHDOG_ENABLED`が無効) / domain watchdog is not enabled on this instance".to_string()
    } else {
        format!(
            "{healthy_count} / {total_watched} ホストが直近のチェックで正常応答 / {healthy_count} of {total_watched} monitored hosts responded healthy on the last check"
        )
    };

    let html = format!(
        r#"<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<title>open-web-server デモ / Demo</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  body {{ font-family: system-ui, sans-serif; max-width: 860px; margin: 2rem auto; padding: 0 1rem; line-height: 1.6; color: #1a1a1a; }}
  h1 {{ font-size: 1.5rem; }}
  h2 {{ font-size: 1.1rem; margin-top: 2rem; border-bottom: 1px solid #ccc; padding-bottom: 0.3rem; }}
  table {{ border-collapse: collapse; width: 100%; margin-top: 0.5rem; }}
  th, td {{ border: 1px solid #ddd; padding: 0.4rem 0.6rem; text-align: left; font-size: 0.92rem; }}
  th {{ background: #f4f4f4; }}
  td.empty {{ color: #777; font-style: italic; }}
  .notice {{ background: #fff8e1; border: 1px solid #e0c96a; border-radius: 6px; padding: 0.8rem 1rem; margin: 1rem 0; font-size: 0.92rem; }}
  .badge-ok {{ color: #1a7f37; font-weight: bold; }}
  code {{ background: #f0f0f0; padding: 0.1rem 0.3rem; border-radius: 3px; }}
  footer {{ margin-top: 2.5rem; font-size: 0.82rem; color: #666; }}
</style>
</head>
<body>
<h1>open-web-server デモ環境 / Demo Environment</h1>
<p>
  ここは <strong>open-web-server</strong>(Rust製のWebサーバー/リバースプロキシ本体
  — TLS終端・テナントルーティング・vhost配信・ヘルスチェック監視を1バイナリで
  提供)の、実際に稼働中のインスタンスを<strong>読み取り専用</strong>で
  覗けるデモページです。<br>
  This page lets you inspect a <strong>live, read-only</strong> view of
  <strong>open-web-server</strong> (a Rust-native web server &amp; reverse
  proxy providing TLS termination, tenant routing, vhost serving, and health
  monitoring in a single binary).
</p>
<div class="notice">
  <strong>正直な開示 / Honest disclosure:</strong>
  このデモは本番と同一のバックエンドインスタンスを指すエイリアスです。
  このページ自体は<code>GET</code>のみで構成され、テナント追加・削除・
  vhost変更のような本番の管理操作(<code>POST</code>/<code>DELETE</code>/
  <code>PUT</code> の <code>/admin/*</code> API、管理トークン必須)は
  一切実行できません。表示している内容も、内部ネットワーク構成
  (転送先アドレス・DB接続文字列・サーバー上のファイルパス等)を含まない
  安全な要約に限定しています。<br>
  This demo is an alias pointing at the same backend instance as
  production. The page itself only issues <code>GET</code> requests and
  exposes no path to destructive admin operations (adding/removing
  tenants, changing vhosts — all gated behind the authenticated
  <code>/admin/*</code> API). The data shown is limited to a safe summary
  that excludes internal topology (backend addresses, DB connection
  strings, on-disk paths).
</div>

<h2>テナントルーティング一覧(読み取り専用) / Tenant Routing (read-only)</h2>
<table>
<thead><tr><th>Host</th><th>Backend</th></tr></thead>
<tbody>
{tenant_rows}
</tbody>
</table>

<h2>静的/PHP配信vhost一覧(読み取り専用) / Static/PHP Vhosts (read-only)</h2>
<table>
<thead><tr><th>Host</th></tr></thead>
<tbody>
{vhost_rows}
</tbody>
</table>

<h2>ヘルスチェック状況 / Health Check Status</h2>
<p>{watchdog_summary}</p>
<p><span class="badge-ok">GET /healthz → ok</span>(このインスタンス自体の死活確認 / this instance's own liveness check)</p>

<footer>
  open-web-server —
  <a href="https://github.com/aon-co-jp/open-web-server">GitHub</a>
</footer>
</body>
</html>
"#
    );

    html_response(StatusCode::OK, html)
}
