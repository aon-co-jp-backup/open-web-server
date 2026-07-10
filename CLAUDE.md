# 技術スタック・開発ルール(open-web-server)

このリポジトリ、および関連プロジェクト(`open-runo`/`aruaru-db`/`aruaru-web`/
`open-raid-z`)で開発・保守を行う際は、以下を基本方針とする。作業ドライブは
`F:\open-runo`(E:ドライブは2026-07-10に消失、以後Fが実体)。この節は
[`open-raid-z`](https://github.com/aon-co-jp/open-raid-z) の `CLAUDE.md`
を正本とし、各プロジェクトへコピーして同期する。

## フロントエンド

- **Tauri**(メインフレームワーク): https://v2.tauri.app/ | https://github.com/tauri-apps/tauri
- HTML5 / CSS3
- **TypeScript**: 必要最低限・最小限の範囲に留める(ロジックはRust側に置き、
  TypeScript側はDOM操作・`invoke()`呼び出し等の薄い配線のみとする方針)
- **Bootstrap**

## バックエンド・コア

- **Rust**(メイン言語): https://www.rust-lang.org/ja/ | https://github.com/rust-lang/rust
- **Poem**(Webフレームワーク): https://docs.rs/poem/latest/poem/ | https://github.com/poem-web/poem

## このリポジトリ固有の役割

open-web-server は課金アイテム/金融データの消失防止に特化した Web サーバー。
`open-web-server-wire`(3層防御通信) → `open-web-server-ledger`(冪等WAL+3ホップ
コミット) → `open-runo`(Federation Gateway) → `aruaru-db`(分散Git-on-SQL)の
経路で、二重課金・データ消失を防ぐ。

## API設計思想(参考・概念のみ)

- **VersionLess API**という考え方を参考にする(WunderGraphのブログ/podcast参照)。
- **WunderGraph Cosmo**: あくまで**参考・着想元としてのみ**参照する。
  **実装には絶対に使用しない**。https://github.com/wundergraph/cosmo

## 関連プロジェクト

- **open-runo**: https://github.com/aon-co-jp/open-runo
- **open-web-server**(このリポジトリ): https://github.com/aon-co-jp/open-web-server
- **aruaru-db**: https://github.com/aon-co-jp/aruaru-db
- **aruaru-web**: https://github.com/aon-co-jp/aruaru-web
- **open-raid-z**(開発ルールの正本): https://github.com/aon-co-jp/open-raid-z
- **rs-to-readme**: https://github.com/aon-co-jp/rs-to-readme

## 運用ルール

- **開発中はこの`CLAUDE.md`を、コード変更のコミット/pushと必ず一緒に push する**。
- 実装で迷った場合は、学習データからの推測より公式ドキュメントを優先して参照する。
- 作業ドライブが変わった場合は、この節と関連プロジェクトの引き継ぎ資料を更新する。

## 現状(このリポジトリ固有)

- `cargo check --workspace` は成功する(4クレート構成)。
- 2026-07-10: `open-web-server-ledger` がビルド不能だった問題を修正
  (Cargo.toml に `async-trait`/`chrono` の依存が抜けていた)。冪等性
  ショートサーキットの単体テストを追加(以前はこのクレートにテストが
  0件だった)。
