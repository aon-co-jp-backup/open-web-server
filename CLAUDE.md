# 技術スタック・開発ルール(open-web-server)

このリポジトリ、および関連プロジェクト(`open-runo`/`aruaru-db`/`aruaru-web`/
`open-raid-z`/`poem-cosmo-tauri`)で開発・保守を行う際は、以下を基本方針とする。
作業ドライブは `F:\open-runo`(E:ドライブは2026-07-10に消失、以後Fが実体)。
この節は [`open-raid-z`](https://github.com/aon-co-jp/open-raid-z) の
`CLAUDE.md` を正本とし、各プロジェクトへコピーして同期する
(最終同期: 2026-07-11、open-raid-z側の2026-07-10方針転換を反映)。

## 方針転換(2026-07-10、最終確定)

ユーザー指示により以下へ転換・確定。**Tauri・Poem・WunderGraph Cosmo(有料版
含む)を外部パッケージ/ライブラリとして直接依存させることはしない**。ただし
各ツールが提供する**機能・API形状・体験には互換性を保ち**、Rust標準ライブラリ
+ tokio/hyper で自前実装して置き換える(依存だけを断ち、機能面の互換性は
維持する)。**`poem-cosmo-tauri` と `open-runo` は2リポジトリを同時並行で
開発する**(2026-07-10、再確定)。どちらもTauri/Poemを含まない構成。
実装(例: crates/open-runo-routerのPoem→tokio/hyper移行)はpoem-cosmo-tauri
側で先行させ、動作確認できたファイルをopen-runoへミラーする運用とする。

**このリポジトリ固有の既知ギャップ**: `open-web-server-gateway` クレートは
本節執筆時点(2026-07-11)でまだ `poem`/`poem-openapi` パッケージに直接依存
したままで、tokio/hyper直接実装への移行は未着手。フロントエンド
(aruaru-web ネイティブUI相当)を持たないバックエンド専用リポジトリのため、
上記の「WASMフロントエンド」方針は直接は適用されないが、バックエンドの
Poem依存除去は他リポジトリと同じ方針に従うべき残作業として認識している。

**poem-cosmo-tauri と open-runo の違い(2026-07-11、ユーザー確認済み、
open-raid-z正本より転記)**: 両者は共通コア(Cosmo有料版機能のOSS Rust
再実装)を持つが**全く違うリポジトリのプロジェクト**であり統合対象では
ない。poem-cosmo-tauri はさらに範囲が広く、Poem/Tauriの**全機能を
AI駆動開発で一から自作・再現する**という上乗せ目標を持つ(open-runoには
ない)。詳細は open-raid-z の `CLAUDE.md` を参照。

## フロントエンド(2026-07-10、方針更新)

- Tauriパッケージには直接依存しない。ただしTauriのデスクトップUI体験・
  `invoke()`的なコマンド呼び出しインターフェースとは互換性を保つ。
- **HTML5/CSS3・TypeScript・Bootstrap・Node.jsのスタックは廃止**。
  Rustをメイン言語としてフロントエンドとバックエンドを統合し、
  **WebAssembly (WASM)** に置き換える(コンパイル対象はRust →
  `wasm32-unknown-unknown`)。DOM操作・`invoke()`相当の呼び出しは
  Rust製WASMモジュール側で行い、TypeScript/Node.jsのビルドチェーンには
  依存しない。https://webassembly.org/ | https://rustwasm.github.io/
- open-web-server自体はバックエンド専用リポジトリのため直接のWASM UIは
  持たないが、将来の管理画面(ロードマップ参照)はこの方針に従いRust/WASMで
  実装する(Tauriは使わない)。

## バックエンド・コア

- **Rust**(メイン言語、標準ライブラリ中心): https://www.rust-lang.org/ja/ | https://github.com/rust-lang/rust
- **tokio** + **hyper**(Webフレームワークなしで直接HTTPサーバを自前実装):
  https://tokio.rs/ | https://docs.rs/hyper/latest/hyper/
- Poemパッケージには依存しないが、Poemのルーティング/ハンドラAPI形状とは
  互換性のあるインターフェースを維持しながらtokio/hyper直接実装へ移行する
  (本リポジトリの `open-web-server-gateway` は上記「既知ギャップ」の通り
  この移行がまだ未実施)。

## このリポジトリ固有の役割

open-web-server は課金アイテム/金融データの消失防止に特化した Web サーバー。
`open-web-server-wire`(4層防御通信) → `open-web-server-ledger`(冪等WAL+3ホップ
コミット) → `open-runo`(Federation Gateway) → `aruaru-db`(分散Git-on-SQL)の
経路で、二重課金・データ消失を防ぐ。

### 拡張要件(2026-07-11、ユーザー指示——目標アーキテクチャ、実装は段階的に)

`open-web-server` と `poem-cosmo-tauri`(または `open-runo`)を用い、さらに
**PostgreSQL・`aruaru-db`・`open-raid-z`** を組み合わせて、3Dオンライン
ゲームの課金アイテム・金融データ・証券データ・クレジットカード情報等が
**ネットワーク上で紛失しない**ことを目的とした以下の仕組みを実装する。

**2026-07-11 改訂: 三層三重 → 四層四重へ拡張(ユーザー指示・ネット調査済み)**。
当初「TCP-IP・UDP-IPの三層三重通信」としていたが、ユーザーの指摘により
2026年時点の通信・DB冗長化のより現実的な到達点を調査した結果、
「TCP+UDPの2種類だけ」は最新の実践として十分ではなく、以下の**4層4重**
構成が実在の技術で裏付けられると判断し、目標を書き換える(出典は本節末尾)。

1. **VersionLessAPI(バージョンレス)とバージョン管理のハイブリッド**:
   エンドポイント自体はVersionLessAPI(open-runo/poem-cosmo-tauriのcosmo
   部分が既に提供)としつつ、データそのものは `aruaru-db`(Git-on-SQL、
   コミット単位の変更履歴)でバージョン管理する——APIはバージョンレス、
   データは完全な変更履歴を保持、というハイブリッド。(変更なし)
2. **Git管理 + ディスク冗長化基盤(2026-07-11、ZFS↔DATABASEの関連性を
   ネット調査の上で追記)**: `aruaru-db` の Git-on-SQL 特性を使い、課金
   アイテム/金融/証券データの全変更をコミットとして記録し、任意時点への
   復元・差分監査を可能にする。`open-raid-z`(RAID-Z2/Z3相当のディスク
   冗長化、実体はZFS類似設計)をこのGit履歴・データ本体の永続化層の冗長化
   基盤として組み合わせる。**ZFS系ストレージとDATABASE(PostgreSQL/
   aruaru-db)の読み書きには実務上の裏付けのある関連性があり、単なる
   「ディスク冗長化」以上の直接的なメリットがある**:
   - **チェックサムの多層防御**: ZFSは全ブロックを読み取り時に検証し、
     ext4等では見逃されるサイレント破損を検出できる。PostgreSQL自身の
     チェックサムとは異なる障害モードを捕捉するため、両方を有効にする
     ことが推奨される([PostgreSQL on ZFS: Tuning, Snapshots,
     Pitfalls](https://sumguy.com/postgresql-on-zfs-tuning-snapshots/))。
   - **Copy-on-Writeによるpartial write対策**: ZFSは既存データを
     決してその場で上書きしないため、部分書き込み(torn page)への
     天然の防御になる——ZFSの上で動かす場合、PostgreSQLの
     `full_page_writes`を無効化しても安全という実務知見がある(同上)。
   - **ZIL/SLOGによる同期書き込みの耐久性**: ZFS Intent Log(ZIL)は
     同期書き込み(DBのトランザクションコミットが典型例)のデータを
     確定応答前にログへ書き込み、専用の高速デバイス(SLOG)で
     オフロードできる——PostgreSQLのWAL・aruaru-dbのコミット確定という
     「同期書き込みで確実性を保証する」場面と直接関係する
     ([Workload Tuning, OpenZFS
     documentation](https://openzfs.github.io/openzfs-docs/Performance%20and%20Tuning/Workload%20Tuning.html))。
   - **recordsizeチューニングによる書き込み増幅の回避**: ZFSは
     recordsize単位でCopy-on-Writeするため、DBの実際のページ/ブロック
     サイズ(PostgreSQLは8KB等)とrecordsizeが不一致だと書き込み増幅が
     発生する。DBデータセットのrecordsizeをDB側のブロックサイズに
     合わせることが推奨される
     ([ZFS on Postgres: Recordsize Mismatch and Write
     Amplification](https://tech-champion.com/database/postgresql/zfs-on-postgres-recordsize-mismatch-and-write-amplification/))。
   - **aruaru-db(Git-on-SQL)とZFSスナップショット/クローンの概念的親和性**:
     ZFSのスナップショットはCopy-on-Writeブロックへの参照であり、
     `zfs clone`はスナップショットを起点に分岐(Gitのブランチに相当する
     操作)でき、`zfs send`/増分sendでリモートプールへ複製できる——
     Gitのコンテンツアドレス指向な履歴管理とは実装が異なるが、
     「変更前のブロックを保持したまま新しい版を作る」という設計思想は
     共通する([Using ZFS to Version Control Large
     Datasets](https://gist.github.com/CMCDragonkai/1a4860671145b295fe7a4d8bc3968e87)、
     [ZFS Essentials: Copy-on-write &
     Snapshots](https://www.open-e.com/blog/copy-on-write-snapshots/))。
     これにより、aruaru-dbのGitコミット履歴(アプリケーション層の版管理)
     とは**独立した**、ファイルシステム層のスナップショット/複製という
     もう1系統の冗長性を追加できる——片方(Gitコミット履歴)が壊れても
     もう片方(ZFSスナップショット)から復元できる、という二重化。
     **調査結果と次回新規開発予定(2026-07-11、ユーザー判断)**: aruaru-dbが
     使うRaft分散合意によるレプリケーションとZFSスナップショットを直接
     統合する確立された技術・実装は調査時点(2026-07-11)で既存の文献からは
     見つからなかった。しかし**これは「確立技術が無いから断念する」では
     なく「新規性のある発見であり、実装可能なので次回パスで新規開発する」
     という判断とする**(ユーザー指示)。具体的な着眼点: aruaru-dbが
     Raftログエントリ(コミット)を確定するタイミングに合わせて`open-raid-z`
     側のZFS類似スナップショットを同期的に(または非同期でベストエフォート
     に)取得することで、アプリケーション層(Gitコミット履歴)とファイル
     システム層(ZFSスナップショット)の**2つの独立した版管理機構の間に
     時刻・トランザクション単位での対応関係**を持たせる——これにより
     「Gitコミット履歴からの復元」と「特定時点のZFSスナップショットからの
     復元」のどちらでも同じ論理的な状態に到達できることを保証する、という
     新規の統合パターンになりうる。既存の確立技術が無い分野なので、
     設計・プロトタイプ・実バイナリでの検証を伴う本格的な開発タスクとして
     次回パス以降で着手する。
   - これらは`open-raid-z`側の実装(スナップショット・チェックサム・
     recordsize相当の設定)を、単なる「下にあるディスク冗長化層」ではなく
     「DB書き込みパスと積極的に協調させるべき層」として扱うべき根拠となる。
     具体的な実装(recordsize設定の露出、ZIL/SLOG相当の同期書き込み
     経路の追加、**aruaru-dbコミットとZFSスナップショットのタイミング連携
     [次回新規開発予定、上記参照]**等)は今後のパスで検討する(今回は
     ドキュメント上の関連性整理のみ、コード変更なし)。
3. **通信層の四重化(TCP-IP・UDP-IP・QUIC/MPQUIC・MPTCPまたはSCTP)**:
   単純な「TCP1系+UDP1系」の二種類ではなく、**性質の異なる4つの伝送方式**
   を並行させることで、単一プロトコル・単一経路の欠陥(輻輳制御の弱さ・
   単一NIC/回線への依存等)からも独立した耐障害性を持たせる:
   - **① TCP-IP**(既存、確実な到達保証が必要な本体データ用途、権威パス)
   - **② UDP-IP**(既存実装済み、低遅延の即時通知・ハートビート用途、
     fire-and-forget)
   - **③ QUIC(理想的にはMultipath QUIC=MPQUIC)**: UDP上に構築されるが
     TLS1.3組み込み・0-RTT再接続・単一コネクション内での複数ストリーム
     多重化を持ち、TCP/UDPどちらとも異なる耐障害特性を持つ第3の伝送方式。
     MPQUICは複数の物理経路(例: Wi-Fi+モバイル回線)に単一のQUIC
     コネクションを分散できる([Multipath QUIC, ACM CoNEXT
     2017](https://dl.acm.org/doi/10.1145/3143361.3143370))。
   - **④ Multipath TCP(MPTCP)またはSCTP(CMT-SCTP)**: 単一の論理
     コネクションを複数の物理ネットワークパス(マルチホーミング)へ
     同時分散させることで、通信「経路」自体の冗長化を担う——①②③が
     主に「プロトコルの性質」による冗長化であるのに対し、④は「物理経路」
     による冗長化という異なる軸を追加する
     ([CMT-SCTP and MPTCP Multipath Transport Protocols: A Comprehensive
     Review](https://www.mdpi.com/2079-9292/11/15/2384))。
   - 既存の`open-web-server-wire`の「4層防御通信」(TLS→相互認証→AEAD
     payload暗号化→seq/timestampリプレイ対策、2026-07-11に3層から4層へ
     拡張)はセキュリティ層であり、この4層4重通信(伝送路の冗長化)とは
     別軸として両立させる(前回HANDOFFの整理を維持)。
4. **DATABASE書き込みの四重化(PostgreSQL・aruaru-db・マルチリージョン
   同期レプリケーション・独立監査ログ)**: 単純な「PostgreSQL+aruaru-dbの
   2箇所」ではなく、以下4つの独立した永続化先へ同一トランザクションを
   反映する:
   - **① PostgreSQL**(ACIDトランザクション保証。金融システムに
     eventual consistencyは許されずACIDが必須、2026年のフィンテック新規
     プロジェクトでもPostgreSQLがデフォルト選択であり続けるとされる —
     [Best Database for Financial Data: Guide
     2026](https://www.ispirer.com/blog/best-database-for-financial-data)
     参照)
   - **② aruaru-db**(Git履歴・分散耐障害性、既存踏襲)
   - **③ マルチリージョン同期レプリケーション**: 地理的に離れた
     リージョン間で同期複製することで、単一データセンター障害からの
     独立性を確保する。同期方式は一貫性を保つがレイテンシとのトレード
     オフがあり、非同期方式は性能面で有利だが障害時のデータ損失リスクが
     残る、というトレードオフを踏まえて選択する
     ([Architecture Strategies for Designing for Redundancy, Microsoft
     Azure Well-Architected
     Framework](https://learn.microsoft.com/en-us/azure/well-architected/reliability/redundancy))。
   - **④ 独立した監査用トランザクションログ**: 金融機関が実際に採用する
     パターンとして、主系とは**別システムの冗長トランザクションログ**を
     保持し、二重処理やデータ不整合の検知・突き合わせに用いる——これは
     ①②③のような「同一データの複製」ではなく「独立した検証手段」という
     異なる役割を持つ第4の永続化先。
   - `open-runo-db::DualBackend`(またはこのリポジトリ固有の同等機構)の
     整合性自動検証・自動修復パターンを、2系統から4系統へ拡張する形で
     踏襲する。

**実装方針**: 上記は目標アーキテクチャ全体像であり、一度に全て実装しようと
せず、段階的に検証可能な単位に分割して進めること。各段階で実バイナリ・
実ネットワーク通信による検証を行い、型チェックのみで「完了」と報告しない。
詳細な進捗は本ファイルのHANDOFF節に記録する。

**進捗(2026-07-11)**: 上記(3)のうち**①TCP-IPと②UDP-IPの実装のみ**完了
(旧「三層三重」目標時点での第一実装、`open-web-server-wire::udp_channel`
+ `open-web-server-ledger::Ledger::enable_udp_redundant_path()`、
UDP側はfire-and-forgetのみで再送機構なし)。**四層四重への拡張により、
③QUIC/MPQUIC・④MPTCP/SCTPは新規追加項目としてまだ未着手**。(1)(2)(4)も
引き続き未着手。次回パスは③または④のどちらか一方の第一実装(既存の①②と
並行動作する追加の伝送路として)から着手するのが妥当。

## API設計思想(参考・概念のみ)

- **VersionLess API**という考え方を参考にする(WunderGraphのブログ/podcast参照)。
- **WunderGraph Cosmo**: パッケージとしては直接依存させない。GraphQL
  Federation / VersionlessAPI というAPI形状・コンセプトのみ参考にし、
  Rust標準+tokio/hyperで互換性を保ちつつ自前実装する。
  https://github.com/wundergraph/cosmo

## 関連プロジェクト

- **open-runo**(poem-cosmo-tauriと同時並行開発。2026-07-10付けで開発再開):
  https://github.com/aon-co-jp/open-runo
- **open-web-server**(このリポジトリ): https://github.com/aon-co-jp/open-web-server
- **aruaru-db**: https://github.com/aon-co-jp/aruaru-db
- **aruaru-web**: https://github.com/aon-co-jp/aruaru-web
- **open-raid-z**(開発ルールの正本): https://github.com/aon-co-jp/open-raid-z
- **rs-to-readme**: https://github.com/aon-co-jp/rs-to-readme
- **poem-cosmo-tauri**(open-runoと同時並行開発。Poem→tokio/hyper移行の
  実装先行地点): https://github.com/aon-co-jp/poem-cosmo-tauri

## 運用ルール

- **開発中はこの`CLAUDE.md`を、コード変更のコミット/pushと必ず一緒に push する**。
- 実装で迷った場合は、学習データからの推測より公式ドキュメントを優先して参照する。
- 作業ドライブが変わった場合は、この節と関連プロジェクトの引き継ぎ資料を更新する。
- **無人自動開発(確認不要・自動デバッグ)のタイミングでは、スケジュール実行待ちに
  せず、1パス内でできる限り連続して作業を進める**こと。小さく検証可能な単位
  (1ハンドラ/1機能ごとに `cargo test` → commit → push)を保ちながらも、
  次の増分に進む前にバックグラウンド待機で止まらない。
- **各無人開発パスの最後には、必ず以下を実行すること**(ユーザー指示、
  2026-07-11): (1) 世界10ヶ国語のREADME(`README-<言語>.md`、日本語・
  英語・中国語簡体字・韓国語・スペイン語・フランス語・ドイツ語・
  イタリア語・ロシア語・アラビア語)を、そのパスで行った実装内容に
  合わせて更新する、(2) 他プロジェクトへのお引越し可能ファイル
  (`PORTING.md`)を同様に最新の実装内容に合わせて更新する、(3) この
  開発環境ルール(`CLAUDE.md`)のHANDOFF節・現状節を更新して保存する、
  (4) 上記(1)〜(3)を含む全ての変更をコミットしてpushする。この4点を
  終えてから、そのパスを終了する。
- **ドキュメント(parity調査・ギャップ一覧・HANDOFF等)に「未調査」「未着手」
  「意図的に先送り」「将来検討」等と記録されている項目を見つけた場合、
  それは承認待ちの提案ではなく、次に着手すべき実装対象そのものである**
  (ユーザー指示、2026-07-12)。「ご指示はありますか?」のように追加の
  確認を求めて手を止めず、確認不要でそのまま実装を進めること。
  優先度・実現可能性の判断は自分で行ってよいが、「未着手だから今回は
  見送る」という判断そのものをユーザーへの確認なしに下してはならない
  ——見送る場合も、まず着手を試み、真に不可能/著しく非現実的と判明した
  場合のみ、その理由をドキュメントに明記した上で次の項目に進む。

## 現状(このリポジトリ固有)

- `cargo check --workspace` / `cargo test --workspace` は成功する(4クレート構成、
  2026-07-11時点で全18テストがgreen: gateway 4件 / ledger 3件 / wire 11件
  [3層防御通信→4層防御通信への拡張でreplay_guardの5件が追加])。
- 4クレートの実装(`core`/`wire`/`auth`/`payload_crypto`/`tls`/`ledger`/`gateway`の
  各handler・middleware)はスタブなし。`todo!()`/`unimplemented!()`/`TODO`/`FIXME`は
  リポジトリ全体で0件(2026-07-11巡回時点でも再確認済み)。`handlers/wal.rs` の
  `InMemoryWal` は本番実装(sled/RocksDB/aruaru-db)への差し替え前提の参照実装で
  あることをdocコメントで明示済み — これは「隠れたスタブ」ではなく意図した設計。
- `open-web-server-gateway` に OpenTelemetry 連携(`src/telemetry.rs`)を追加済み
  (2026-07-11)。`grant_item`/`charge` ハンドラがスパン化され、
  `OTEL_EXPORTER_OTLP_ENDPOINT` の有無で OTLP/HTTP エクスポートと標準出力
  フォールバックを切り替える。テストはインメモリエクスポータで検証。

## HANDOFF (直近の自動巡回ログ、上が最新)

- **2026-07-11 (今回、open-web-server-wireを3層防御通信→4層防御通信へ拡張、
  ユーザー指示)**: セキュリティレイヤー(TLS/相互認証/AEAD payload暗号化)の
  3層構成に、第4層として **リプレイ(再送)対策** を追加。
  **背景**: 第3層のAEAD (`payload_crypto::PayloadCipher`) は改ざん検知・
  機密性は提供するが、正規に暗号化された暗号文をネットワーク上で捕捉し
  そのまま再送する「リプレイ攻撃」は防がない。課金アイテム付与・決済確定の
  ような非冪等操作が暗号文の単純再送だけで二重適用される恐れがあった。
  **実装**: `crates/open-web-server-wire/src/replay_guard.rs` を新規作成。
  `ReplayGuard`(BTreeSetでのシーケンス番号追跡+タイムスタンプ鮮度検証、
  許容窓30秒、追跡上限10,000件で古いものから破棄)と、第3層+第4層を1本に
  まとめた `SecureChannel`(ワイヤーフォーマット
  `seq:u64(BE) || timestamp:u64(BE) || nonce:12B || ciphertext`、seq/timestamp
  をAEADのAssociated Dataとして暗号文に暗号学的に紐付けるため、攻撃者が
  seq/timestampだけを書き換えて再送してもAEADタグ検証で失敗する)を実装。
  `lib.rs` のモジュールdocを3層→4層の図に書き換え、`replay_guard` を
  `pub mod`として追加・`ReplayGuard`/`SecureChannel`を再エクスポート。
  **テスト(実暗号処理での検証)**: 単体テスト5件を追加——正常系round-trip、
  同一フレームの単純リプレイ拒否、タイムスタンプ許容窓外(1970年)の拒否、
  seqバイト改ざんによるAEADタグ検証失敗(AAD紐付けの実証)、鍵不一致での
  復号失敗。`cargo test -p open-web-server-wire` で新規5件を含む全11件
  green、`cargo build -p open-web-server-wire` も成功を確認。
  **ドキュメント**: `docs/architecture.md` の全体構成図・「4層防御通信」節
  (旧「3層防御通信」節)・冗長化伝送経路節を更新。`README.md`(ルート)・
  `README-Japan.md`・`README-English.md`・`README-Chinese.md`・
  `README-France.md`・`README-Germany.md`・`README-Italy.md`・
  `README-Korea.md`・`README-Russia.md`・`README-Spain.md`・
  `README-Arabic.md`(世界10ヶ国語+日本語の全11ファイル)を4層防御通信に
  合わせて更新。`PORTING.md` の該当節も更新。
  **次回実装予定として追記(ユーザー指示、今回は文書化のみ・コード変更なし)**:
  ZFS(`open-raid-z`)のチェックサム・Copy-on-Write・スナップショット特性と
  PostgreSQL/aruaru-dbのACID特性を「RAID(ディスク冗長化)層でも積極的に
  生かす」具体アイデアを次回パスで実装検討する。本節上部の拡張要件(2)に
  既にZFS↔DB関連性の調査結果と方向性(aruaru-dbコミットとZFSスナップショット
  のタイミング連携)を記載済みであり、次回はその**新規開発**(設計・
  プロトタイプ・実バイナリでの検証)に着手する。
  **未実施**: `open-web-server-gateway`/`open-web-server-ledger`側で
  `SecureChannel`/`ReplayGuard`を実際に呼び出す配線(現状は
  `open-web-server-wire`クレート単体での提供のみ)。次回以降の候補。
- **2026-07-11 (UDP-IP冗長経路の第一実装)**: 拡張要件(3)「TCP-IP・
  UDP-IPの三層三重通信」のうち、UDP側の最初の具体的な実装を追加。
  **実装**: `crates/open-web-server-wire/src/udp_channel.rs` を新規作成。
  データグラム形式は `[seq:u64][HMAC-SHA256タグ:32B][PayloadCipher
  ciphertext(nonce12B+AEAD)]`。送信は `UdpSender::send_mutation`
  (fire-and-forget、再送なし)、受信は `UdpReceiver::recv_mutation`
  (HMAC検証→AEAD復号→`Deduplicator`によるIdempotencyKeyデデュープ)。
  `open-web-server-ledger::Ledger` に `enable_udp_redundant_path(bind_addr,
  dest, keys)` (任意呼び出しのasyncビルダー) を追加し、`commit()` の
  `fire_udp_redundant_notice()` が WAL先行書き込み直後に `tokio::spawn` で
  UDP送信を非同期発火、TCP経由の権威パス(既存の`forward_with_retry`)は
  一切ブロックしない設計。**スコープの限界(正直な記載)**:
  UDPの再送機構は実装せず「送りっぱなしの即時通知」のみ。副系TCP(TCP2系)は
  未実装。受信側の実配置(open-runo側でのUDP listenとaruaru-db WALとの結合)
  は別リポジトリのスコープで今回未着手 — 本リポジトリはUDP送信側の結線と、
  検証用の受信ロジック(`UdpReceiver`)の提供までに留まる。
  **テスト(実ソケット・実ネットワーク)**: `open-web-server-wire` に単体
  テスト6件を追加(フレームのround-trip、改ざん検知、鍵不一致拒否、
  デデュープ、および `tokio::net::UdpSocket` を `127.0.0.1:0` に実バインドし
  実送受信する結合テスト2件——1件は正常系の暗号化/HMAC/デデュープの実証、
  もう1件は未listenの宛先へ送ってもハング・panicしないことの実証)。
  `open-web-server-ledger` にも実TCPソケットのモックopen-runoサーバ
  (`tokio::net::TcpListener`、固定JSONレスポンス)を使った統合テスト2件を
  追加: (a) UDP経由の通知がTCP確定コミットと並行して正しく届きデデュープ
  される実証、(b) UDP宛先(`127.0.0.1:1`、未listen)が完全に到達不能でも
  TCP経由の権威パスは`tokio::time::timeout(5s)`内で問題なくコミット成功する
  実証(UDP障害がTCP経路を一切ブロック・破壊しないという設計保証の直接検証)。
  `cargo check --workspace` / `cargo test --workspace` は全13テストgreenを
  確認(gateway 4 / ledger 3 / wire 6)。
  **未実施(正直な記載)**: 実バイナリ(`open-web-server-gateway`等)の
  プロセス起動によるエンドツーエンド検証は、本リポジトリに
  Makefile/docker-compose等の実行手順が無く、`aruaru-db`/PostgreSQL相当の
  依存インフラもこの環境には無いため未実施。上記のクレートレベル実UDP
  ソケット統合テストをその代替とした。
  **ドキュメント**: `docs/architecture.md` に「冗長化された伝送経路」節を
  追加(4層防御通信=セキュリティレイヤーとの違いを明記)。
  `crates/open-web-server-wire/src/lib.rs` のモジュールdocに
  `udp_channel` との関係を追記。この`CLAUDE.md`の拡張要件(3)節に
  進捗ノートを追加(他3項目は未着手のまま明記)。
  **次回以降の候補**: 副系TCPの追加、UDP側の再送/ACK機構(必要性を再検討の
  うえ)、open-runo側でのUDP受信リスナー実装との結合。
- **2026-07-11**: git健全性を確認(壊れたref修復済み、`origin/main` の
  `2fd70a4` を正しく追跡、作業ツリークリーン)。`cargo check --workspace` /
  `cargo test --workspace --no-run` / `cargo test --workspace` すべて成功を再確認。
  `todo!()`/`unimplemented!()`/TODO/FIXME/stub/placeholder を再走査し0件を再確認。
  **実装**: `open-web-server-gateway` に OpenTelemetry 連携を追加。
  `crates/open-web-server-gateway/src/telemetry.rs` で `SdkTracerProvider` を構築し、
  `OTEL_EXPORTER_OTLP_ENDPOINT` が設定されていれば OTLP/HTTP (protobuf) へ、
  未設定なら `opentelemetry-stdout` で標準出力へスパンをエクスポートするよう
  切り替える構成にした。`tracing-opentelemetry` レイヤーを `tracing_subscriber`
  の `registry()` に `fmt` レイヤーと併せて登録し、`main.rs` はプロセス終了直前に
  `TelemetryGuard::shutdown()` でバッファをフラッシュする。`grant_item`
  (`handlers/items.rs`)・`charge`(`handlers/transactions.rs`)の各ハンドラに
  `#[tracing::instrument]` を追加(`#[handler]` の下に置く必要がある点に注意 —
  属性マクロは下から上へ適用されるため、`#[handler]` が先に素のasync fnではなく
  instrument後の関数を見るようにする)。動作検証用の単体テストを追加
  (`telemetry::tests::spans_are_recorded_and_exported_with_service_resource`):
  `opentelemetry_sdk` の `InMemorySpanExporter`(`testing` feature、dev-dependency)
  でネットワーク送信なしにスパン生成・Resource属性付与・エクスポートを検証。
  `cargo test --workspace` で新規テスト含め全件パス。
  **ドキュメント**: `docs/architecture.md` に「可観測性」節を追加。
  `README.md`(ルート)に4本目の柱として一言追記。`README-Japan.md`/
  `README-English.md`(この2つが唯一ロードマップ節を持つ詳細版)を更新し、
  OpenTelemetryのロードマップ項目を完了([x])にし、管理画面ロードマップ項目の
  「Tauri製」という古い表記を2026-07-10のスタック転換に合わせて「Rust→WASM製」
  に修正。他8言語版(`README-France.md`等)は元々ロードマップ節を持たない短縮版
  であり、今回の変更で不正確になる記述が無かったため変更していない。
  この`CLAUDE.md`のフロントエンド/バックエンド節をopen-raid-z側の2026-07-10
  最新版と同期し、関連プロジェクトに`poem-cosmo-tauri`を追加。
  **既知の残課題として明記**: `open-web-server-gateway` は依然として `poem`/
  `poem-openapi` に直接依存しており、tokio/hyper直接実装への移行(open-raid-z
  方針)はまだ未着手 — 次回以降の巡回候補として残す(スコープが大きいため
  今回は着手せず、GraphQLエンドポイント追加時にPoem脱却を併せて検討するのが
  効率的と判断)。
- **次回の巡回で見るべき点**: (1) `open-web-server-gateway` のPoem依存除去
  (tokio/hyper直接実装への移行、open-raid-z方針)。(2) GraphQLエンドポイント
  追加(`async-graphql` 等、Poem脱却と同時に検討すると手戻りが少ない)。
  (3) `open-cosmo` 共通クレート切り出しは、着手前に必ず `open-runo`/
  `aruaru-db` 側のCLAUDE.md HANDOFFログを確認し、互換性のある地ならしが
  既にあるかを確認すること(未確認のまま単独で着手しない)。(4) 管理画面
  (Rust→WASM、Tauriではない)。(5) OpenTelemetryは本リポジトリ側は実装済みだが、
  `open-runo`/`aruaru-db` 側が同じTrace Contextを伝播・エクスポートするように
  なって初めてエンドツーエンドのトレースになる — 両リポジトリの対応状況は
  今回未確認なので、次回以降に確認すること。
- 2026-07-10: ビルド/テストは既に green であることを確認
  (`cargo check --workspace` / `cargo test --workspace --no-run` / `cargo test --workspace`
  すべて成功)。リポジトリ全体を `todo!()`/`unimplemented!()`/TODO/FIXME/stub/placeholder
  で走査し、該当0件を確認(実装は完了しており追加のスタブ実装作業は無し)。
  **バグ発見・修正**: ルートの `README.md` が UTF-16LE(BOMなし)で保存されており、
  `file`コマンドで `data`(バイナリ扱い)と判定され、GitHub上で文字化けして表示される
  状態だった(10言語版の `README-*.md` は全てUTF-8で正常)。内容はそのままUTF-8に
  再保存して復旧。加えて、10言語版READMEの「他の言語」リンク行が
  日本語・英語版には無く、他8言語版は日本語/英語の2つしかリンクしていなかったため、
  全10ファイルで10言語すべてへの相互リンクに統一。CLAUDE.mdにこのHANDOFF節を追加。
  コミット・push済み(このコミットハッシュは `git log` 参照)。
- 2026-07-10: `open-web-server-ledger` がビルド不能だった問題を修正
  (Cargo.toml に `async-trait`/`chrono` の依存が抜けていた)。冪等性
  ショートサーキットの単体テストを追加(以前はこのクレートにテストが
  0件だった)。
