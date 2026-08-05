//! open-web-server: バイナリエントリポイント。
//!
//! 実体は`lib.rs`の`run()`(2026-08-05、iOSブリッジcrateからも同じ
//! `run()`をFFI経由で呼べるようにするため、ロジック本体をライブラリ側へ
//! 移設した)。このバイナリは`#[tokio::main]`ランタイムを立てて`run()`を
//! 呼ぶだけの薄いラッパー。

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    open_web_server_gateway::run().await
}
