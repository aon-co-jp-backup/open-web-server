//! open-web-server: バイナリエントリポイント。
//!
//! 実体は`lib.rs`の`run()`(2026-08-05、iOSブリッジcrateからも同じ
//! `run()`をFFI経由で呼べるようにするため、ロジック本体をライブラリ側へ
//! 移設した)。このバイナリは`#[tokio::main]`ランタイムを立てて`run()`を
//! 呼ぶだけの薄いラッパー。

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `--version`は自己アップデート機構(`auto_update.rs`)がダウンロード
    // した新バイナリの起動可否を確認するために使う(open-easy-web版と
    // 同じ「壊れたバイナリで本番を巻き込まない」ためのサニティチェック)。
    // すぐ標準出力へバージョンを書いて終了し、通常のサーバー起動処理は
    // 一切行わない。
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("open-web-server {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    open_web_server_gateway::run().await
}
