# 開発方針・開発環境ルール(全リポジトリ共通ヘッダー、2026-07-15追記)

## 1. 比較的新しい言語・フレームワークの参照資料一覧

Rust自体は歴史があるが、本エコシステムが採用する **Poem** のような
比較的新しい・情報量がまだ少なめのWebフレームワークは、Python+FastAPIの
ような広く普及した組み合わせと比べ、AIモデルの学習データ・公開されている
実装例/Q&A/ブログ記事の絶対量が少ない傾向がある。そのため、AI駆動開発
(Claude等)がこれらを扱う際、実装の勘違い・API名の記憶違い・古いバージョン
のAPIでの実装(本プロジェクトで実際に複数回発生した既知の失敗パターン)に
よる**手戻り・いたちごっこ**が起きやすい。

対策として、AIが作業を始める際は、以下から**そのタスクに必要な部分だけ**を
先に参照してから実装に着手すること(全部読む必要はない。関連しそうな1〜2件を
拾い読みする程度で十分)。これにより歩留まりが上がり、AI駆動開発の手戻りが
減ることが期待される。

| 技術 | 公式ドキュメント | GitHub | 補足・ブログ等 |
|---|---|---|---|
| Rust言語本体 | https://doc.rust-lang.org/book/ | https://github.com/rust-lang/rust | https://blog.rust-lang.org/ |
| Poem(Webフレームワーク) | https://docs.rs/poem/latest/poem/ | https://github.com/poem-web/poem | https://crates.io/crates/poem |
| Tokio(非同期ランタイム) | https://tokio.rs/tokio/tutorial | https://github.com/tokio-rs/tokio | https://tokio.rs/blog |
| async-graphql | https://async-graphql.github.io/async-graphql/en/index.html | https://github.com/async-graphql/async-graphql | https://crates.io/crates/async-graphql |
| Tauri | https://tauri.app/ | https://github.com/tauri-apps/tauri | https://tauri.app/blog/ |
| wasm-bindgen / web-sys | https://rustwasm.github.io/wasm-bindgen/ | https://github.com/rustwasm/wasm-bindgen | https://rustwasm.github.io/docs/book/ |
| SurrealDB | https://surrealdb.com/docs | https://github.com/surrealdb/surrealdb | https://surrealdb.com/blog |
| sqlx | https://docs.rs/sqlx/latest/sqlx/ | https://github.com/launchbadge/sqlx | |
| WinFsp | https://winfsp.dev/ | https://github.com/winfsp/winfsp | |
| DirectX 12 / DirectML | https://learn.microsoft.com/en-us/windows/win32/direct3d12/directx-12-programming-guide | https://github.com/microsoft/DirectML | https://devblogs.microsoft.com/directx/ |
| WebAssembly(wasm32全般) | https://webassembly.org/ | https://github.com/WebAssembly | https://rustwasm.github.io/docs/book/ |

> ⚠️ **重要な注意(正直な開示)**: このURL一覧は、Web検索ツールを持たない
> セッションで学習データに基づき記載したものであり、**実在性・現在の
> 有効性・記載内容の正確性を検証していない**。特にAI(Claude含む)が
> このリストを鵜呑みにして実装や回答の根拠にすることは避け、
> **開発者自身が実際にアクセスして確認する**か、Web検索が使える
> セッションで一次情報を再確認してから利用すること。リンク切れ・
> リダイレクト・バージョン変更(特にAPIの破壊的変更)の可能性を
> 常に考慮する。新しい技術を追加する場合はこの表に追記していくこと。

## 2. AI駆動開発ツールに関する所感(2026-07-15、ユーザー所感として記録)

2026-07-15時点、ChatGPT等の汎用AIチャットは小規模なWebアプリ程度までは
開発できるものの、システムがある程度複雑・大規模になると出戻りが大きくなり、
一度に扱えるプログラムサイズにもすぐ限界が来る傾向がある。

Claude Code / Claude Desktopは、ローカルドライブを直接指定してファイルの
読み書きができ、GitHubリポジトリの読み出し(本プロジェクトのような
複数リポジトリにまたがるエコシステム)にも対応できるため、本プロジェクトの
ような規模のAI駆動開発には適していると考えられる。新しくAI駆動開発環境を
セットアップする際の選択肢として推奨する。

---

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

### パフォーマンス・並行処理方針(2026-07-13、ユーザー指示)

システム全体として、4層4重の通信・DB冗長化によるハイセキュリティを
保ちつつ、ハイパースレッディング/マルチコア/マルチスレッドを活かした
高速性を両立させる。**非同期(tokio、マルチスレッドランタイム)を基本**
とし、必要な場面(CPU負荷の高い計算・厳密な順序保証が必要な処理等)での
み同期処理を用いる。実装はRust + tokio/hyper(Poem互換のAPI形状は
維持しつつ、`poem`パッケージには直接依存しない——既存方針の通り)を
中心とする。具体的な着眼点: (1) `#[tokio::main]`のランタイムflavorが
誤ってcurrent_threadに固定されていないか確認する、(2) async関数内で
同期I/O(`std::fs::*`等)やCPU負荷の高い処理を直接呼ばず、
`tokio::task::spawn_blocking`へ退避する、(3) チェックサム計算・圧縮・
統計処理等のCPU律速な処理は`rayon`等によるデータ並列化を検討する、
(4) セキュリティクリティカルなホットパス(レート制限・リプレイ対策等)の
排他ロックが並行スループットのボトルネックになっていないか確認する。

## このリポジトリ固有の役割(2026-07-13、要約を統合・整理)

open-web-server は、3Dオンラインゲームのアイテム課金や、クレジットカード
決済のような金融データを扱う、24時間365日ノンストップ運用の
ミッションクリティカルな Web サーバー。**Rust + tokio/hyper**(Poemには
直接依存しない、2026-07-10のスタック転換済み。ルーティング/ハンドラの
API形状はPoem互換)で実装し、**4層防御通信による高セキュリティと
高速性の両立**、および**ZFS互換(open-raid-z)とACID互換(PostgreSQL)の
ハイブリッド技術**を核として、`aruaru-db`・`open-raid-z`・`open-runo`と
連携する多層防御アーキテクチャにより、ネットワーク瞬断・プロセス
再起動・リトライが起きても「二重課金」も「データ消失」も起こさない
設計を実現する。

`open-web-server-wire`(4層防御通信: TLS 1.3 → 相互認証 → AEAD
ペイロード暗号化 → seq/timestampリプレイ対策)→
`open-web-server-ledger`(冪等WAL + 3ホップコミット + 独立監査ログ +
マルチリージョン同期レプリケーション)→ `open-runo`(Federation
Gateway)→ `aruaru-db`(分散Git-on-SQL、ZFS互換スナップショット連携
込み)の経路で、二重課金・データ消失を防ぐ。詳細な到達状況は下記
「拡張要件」節を参照。

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

**進捗(2026-07-13更新、監査ログ実装後)**: (4)の**④独立監査
トランザクションログを実装**(`open-web-server-ledger::audit_log::
FileAuditLog`)。PostgreSQL/aruaru-db/マルチリージョン同期レプリケーション
のいずれとも技術的に独立した追記専用ファイルへ、`Ledger::commit()`が
WAL先行書き込み直後に1レコード(SHA-256チェックサム付き)を追記する。
`scan_and_verify()`でサイレント破損を検出、`reconcile()`でWAL側の確定
済みキー集合と突き合わせて「監査ログにあるがWAL未確定」「監査ログ内で
同一キー重複」を検出できる。`Ledger::enable_audit_log(path)`で任意
有効化、書き込み失敗は権威パスをブロックしない(UDP冗長経路と同じ設計
方針)。実ファイルI/O・チェックサム破損検出・突き合わせレポート・
`Ledger::commit`経由の統合の計4テストで実証済み(`cargo test -p
open-web-server-ledger`で13件中4件が新規)。(4)の①PostgreSQL(前回実装、
未検証)・②aruaru-db(未着手)・③マルチリージョン同期レプリケーション
(未着手)・④独立監査ログ(今回実装)という状態。
**進捗(2026-07-13、通信層の四重化)**: 上記(3)のうち①TCP-IP・②UDP-IPに加え、
**③QUICを実装**(`open-web-server-wire::quic_channel`、`quinn`クレート、
実TLS1.3ハンドシェイク+双方向ストリームの実UDPソケット結合テストで検証済み。
単一経路QUICであり、MPQUICへの拡張は範囲外)。
**④MPTCP/SCTPは調査の上、正直なブロッカーと判断し代替実装を追加**
(`open-web-server-wire::mptcp_channel`)。このWindows開発環境では
カーネルMPTCP(Windowsにネイティブサポート無し)・カーネルSCTP
(主要Rustクレート`lksctp`/`sctp-rs`/`tokio-sctp`はいずれもLinux
`lksctp-tools`前提、Windows版`sctp-sys`は「実験的」ドライバ依存)の
いずれも実ソケット検証が不可能であることを確認した。そのため、
同じ目的(物理経路マルチホーミングによる伝送路冗長化)をユーザー空間で
実現する`aggligator`/`aggligator-transport-tcp`クレート(公式docに
"serves the same purpose as Multipath TCP and SCTP... completely
implemented in user space"と明記)を採用し、実ループバックTCPソケット上の
集約接続でのラウンドトリップ結合テストで検証した。**これは本物の
カーネルMPTCP/SCTPではない**——調査の詳細・判断根拠は
`mptcp_channel`モジュールdocに明記。
上記(4)のうち**①PostgreSQLのWAL実装を追加**(`open-web-server-ledger::PostgresWal`、
`sqlx`クレート、実`BEGIN`/`COMMIT`トランザクション境界。**ただしこの
サンドボックス環境には到達可能なライブPostgreSQLが無く実DB接続検証は
未実施**——SQL構築ロジックの単体テストと`DATABASE_URL`設定時のみ動く
`#[ignore]`統合テストで検証可能性を確保)。**④独立監査ログ・
③マルチリージョン同期レプリケーションも実装完了**
(`open-web-server-ledger::audit_log::FileAuditLog`・`multi_region::
MultiRegionReplicator`、実SQLiteリージョン2つへの同期書き込み+
厳格/N-of-M縮退の両障害ポリシーを実I/Oで検証済み、詳細は上記進捗ノート
参照)。**これで(4)は概念上4系統(①②③④)すべて実装済み**
(①のみ実PostgreSQL接続での検証は未実施だったが、**2026-07-13の別パスで
WSL2上のライブPostgreSQLに対して実接続検証済みとなった**——詳細は本節
末尾の同日付HANDOFFエントリ参照。これで(4)は①②③④すべて実装・検証済み)。
(1)は書き込み側は既に
機能済み、読み出し側(commit_id指定クエリ)が未着手(調査済み・下記参照)。
**(2)のaruaru-db×ZFSスナップショット連携は aruaru-db側で第一段実装を
完了**(`aruaru-dist::snapshot_pairing`+`raid_z_backend`、詳細は
aruaru-db側のCLAUDE.md HANDOFF参照)。残るは(1)の読み出しAPIのみ。
**(1)について今回調査した結果(未着手のまま、理由を明記)**:
`Ledger::commit()`のTCP経由フォワード先(`forward_once`)は現状
`open-runo`のHTTPエンドポイントへのモックであり、本リポジトリ単体には
実際のaruaru-db連携コードが存在しない(`open-runo`/`aruaru-db`側の
実装を跨ぐ)。`MutationReceipt.db_commit_id`は既に配線されており
(aruaru-db発行のcommit_idをそのままクライアントへ返す設計、上記
`forward_once`の`db_commit_id`必須チェック参照)、これ自体はVersionLessAPI
+ Git版管理ハイブリッドの「書き込み側」の実質的な配線と言える。しかし
「commit_idを指定して過去状態を問い合わせる」読み出し側のクエリ
API(拡張要件(1)が真に求める新規ギャップ)は`open-web-server`側に
一切存在しない——`open-web-server-gateway`のハンドラ一覧
(`grant_item`/`charge`等)を確認したが、GET系の状態照会エンドポイント
自体がまだ無く、追加するには`open-runo`側のFederation Gateway経由で
aruaru-dbへの読み出しルートを新設する必要がある(open-runo/aruaru-db
側の実装を要する2リポジトリ以上の作業)。1パスで安全に検証可能な単位に
収めるため今回は見送り、次回パスで`open-web-server-gateway`に
`GET /internal/db/state/:target/at/:commit_id`相当のハンドラ第一実装
(open-runo側の対応する読み出しエンドポイントとセットで)に着手する。

## API設計思想(参考・概念のみ)

- **VersionLess API**という考え方を参考にする(WunderGraphのブログ/podcast参照)。
- **WunderGraph Cosmo**: パッケージとしては直接依存させない。GraphQL
  Federation / VersionlessAPI というAPI形状・コンセプトのみ参考にし、
  Rust標準+tokio/hyperで互換性を保ちつつ自前実装する。
  https://github.com/wundergraph/cosmo

## 契約不要の独自AI(open-cuda × aruaru-llm SET、2026-07-18追記、正本はopen-raid-z参照)

外部AI事業者との有償契約・APIキー(OpenAI等)を必要としない、自前完結の
AI機能が必要になった場合は、`open-cuda` + `aruaru-llm` のSET構成を標準
として使うこと。詳細は`open-raid-z/CLAUDE.md`の同名節を参照。

## 「分身の術」構成の対象拡大(2026-07-18追記、正本はopen-raid-z参照)

このリポジトリ(`open-web-server`)が先行実装した「分身の術」
(共有バックエンドインスタンスへの動的テナント登録、個別インストール
不要)を、`open-cuda`・`aruaru-llm`・`RPoem`・`RCosmo`・`open-raid-z`・
`aruaru-db`にも適用する。管理は`open-easy-web`側の
`appserver_registration.rs`を拡張して行う想定。現状`aruaru-llm`にのみ
`src/tenants.rs`実装済み、他への展開・`open-easy-web`側の統合は未着手
(次回以降の実装対象)。詳細は`open-raid-z/CLAUDE.md`参照。

## 関連プロジェクト

- **open-runo**(poem-cosmo-tauriと同時並行開発。2026-07-10付けで開発再開):
  https://github.com/aon-co-jp/open-runo
- **open-web-server**(このリポジトリ): https://github.com/aon-co-jp/open-web-server
- **aruaru-db**: https://github.com/aon-co-jp/aruaru-db
- **open-easy-web**(第二のKUSANAGI、ドメイン/サブドメイン簡単登録+HTTPS
  自動監視/発行/更新の易操作ツール。高速化機能は含まない、2026-07-13に
  aruaru-webから分離): https://github.com/aon-co-jp/open-easy-web
- **aruaru-web**(2026-07-13廃止。役割はopen-easyweb(易操作)と
  open-runo/poem-cosmo-tauri(高速化)へ分割継承済み): https://github.com/aon-co-jp/aruaru-web
- **open-raid-z**(開発ルールの正本): https://github.com/aon-co-jp/open-raid-z
- **rs-to-readme**: https://github.com/aon-co-jp/rs-to-readme
- **poem-cosmo-tauri**(open-runoと同時並行開発。Poem→tokio/hyper移行の
  実装先行地点): https://github.com/aon-co-jp/RPoem

### テナント別方針: aruaru.tokyo / audiocafe.tokyo(2026-07-14、ユーザー指示・訂正あり)

- **aruaru.tokyo** — `open-easy-web`用に用意されたドメイン。TenantRegistryへの
  登録対象はこちら。
- **audiocafe.tokyo** — 実在するドメインだが、`open-easy-web`のテナントとして
  登録する対象**ではない**(2026-07-14、ユーザー訂正)。別の使い道として
  以下の構成方針のみ記録しておく(TenantRegistryへの登録は行わない)。
  - **現状(2026-07中旬)**: PHPベース。ApacheがCONOHA VPS上で稼働している
    想定(具体的なポート/構成は未調査)。
  - **当面の方針**: PHPをいきなり置き換えるのではなく、**Apache配下で
    `open-runo`を高速化ミドルウェアとして動かす**構成にする(open-runoは
    高速化担当)。
  - **将来方針**: なるべく早い段階でRust + Poemベースへ移行し、AIによって
    動的に変化するサイトにする。
  - 次回(PC版)調査事項: CONOHA VPS上のApache/PHPの実配置(ポート、
    `sites-enabled`構成)。

## 運用ルール

- **開発中はこの`CLAUDE.md`を、コード変更のコミット/pushと必ず一緒に push する**。
- 実装で迷った場合は、学習データからの推測より公式ドキュメントを優先して参照する。
- 作業ドライブが変わった場合は、この節と関連プロジェクトの引き継ぎ資料を更新する。
- **ローカル作業ドライブ(`F:\open-runo`)上の各リポジトリは、常にリモート
  (GitHub)の最新コミットに追従させておくこと**(`git fetch`/`git pull`を
  こまめに実行する。ローカルにのみ存在する未コミット変更がある場合は、
  上書き前に必ず内容を確認し、必要なら `git stash` で退避してから最新化
  する)。
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
- **技術選定・仕様確認で迷った場合、必要に応じて日本語と英語の両方で
  Google検索し、Claude(自分自身)の知識・推論も動員し、GitHubでも
  調査すること**(ユーザー指示、2026-07-13)。
  学習データからの推測だけに頼らず、実在するクレート・ライブラリの
  現状(バージョン・メンテナンス状況・プラットフォーム対応)や、
  最新の実務知見(2026年時点のベストプラクティス等)を実際に検索して
  裏付けを取ってから実装判断を下す。日本語のみ・英語のみでは見つからない
  情報が言語を変えると見つかることがあるため、両言語での検索を基本とする。
- **よほど確認が必要な場面(重大な破壊的操作・仕様の根本方針転換等)を
  除き、確認を求めて手を止めないこと**(ユーザー指示、2026-07-13)。
  技術選定や実装方法で分からないこと・迷うことがあれば、まず上記の通り
  日本語・英語両方でのGoogle検索・GitHub調査を行い、それでも判断が
  つかない場合は自分の工学的判断で最も妥当な選択をして実装を進める。
  「〜については確認が必要です」と言って作業を止め、ユーザーの回答を
  待つことを既定の振る舞いにしない。
- **ユーザーが開発方針・開発環境ルールを口頭で示した場合、それを記憶だけに
  頼らず必ずこの`CLAUDE.md`(該当リポジトリのHANDOFF/運用ルール/関連
  プロジェクト等の適切な節)に書き込み、その場でコミット/pushすること**
  (ユーザー指示、2026-07-14)。「毎回保存」とは、方針が更新されるたびに
  都度反映することを指し、セッション末尾やHANDOFFタイミングまで
  まとめて後回しにしない。
- **バックグラウンド実行(ビルド・テスト・サブエージェント)を「見失わない」
  ための定期確認と、無人での自動再実行**(ユーザー指示、2026-07-18、
  正本は`open-raid-z/CLAUDE.md`参照)。背景: 実際に発生した事象として、
  (a) サブエージェント並列起動時、完了通知前にタスク管理側のIDが失効し
  `No task found`となった(実作業自体は`git status`/`git diff`で裏取り
  でき正常完了していた——**タスク管理メタデータの消失と実際の作業結果は
  別物**)、(b) サブエージェントが最終応答として実装要約ではなく独り言的な
  テキストのみ返した(これも実際にはファイル変更が完了していた)、
  (c) 長時間ビルドがタイムアウトで打ち切られ`could not compile`相当の
  ログが出たが実際は単なる時間切れだった(タイムアウトを伸ばして再実行
  したら成功)。対応方針: (1) バックグラウンド処理が動いている間は放置
  せず一定間隔で状態を能動的に確認する(無意味な高頻度ポーリングはしない)。
  (2) タスク管理システムの応答を鵜呑みにせず、`git status`/`git diff`・
  ビルド/テストログの実際の中身(本物のコンパイルエラーかタイムアウトに
  よる強制終了(exit code 124/143等)かの区別)・生成物の実在確認で必ず
  裏取りする。(3) 裏取りの結果、作業が実際に失われている/失敗している
  場合は確認を求めず自動的に再実行・修正する。(4) 作業自体は完了して
  おり通知だけ欠落していた場合は、二重実行を避けその旨を記録して先に
  進む。(5) これらの判断はユーザーへの確認なしに自分で行ってよい。

## 現状(このリポジトリ固有)

- `cargo check --workspace` / `cargo test --workspace` は成功する(4クレート構成、
  2026-07-13時点(マルチリージョン同期レプリケーション実装後)で
  全35テストがgreen(+1件は要ライブPostgreSQLの`#[ignore]`): gateway 4件 /
  ledger 17件(postgres_wal SQL構築ロジック4件・audit_log単体3件+
  `Ledger::commit`経由の統合1件・multi_region単体3件+`Ledger::commit`
  経由の統合1件を含む) / wire 14件(quic_channel結合テスト2件・
  mptcp_channel結合テスト1件を含む)。
- 4クレートの実装(`core`/`wire`/`auth`/`payload_crypto`/`tls`/`ledger`/`gateway`の
  各handler・middleware)はスタブなし。`todo!()`/`unimplemented!()`/`TODO`/`FIXME`は
  リポジトリ全体で0件(2026-07-11巡回時点でも再確認済み)。`handlers/wal.rs` の
  `InMemoryWal` は本番実装(sled/RocksDB/aruaru-db)への差し替え前提の参照実装で
  あることをdocコメントで明示済み — これは「隠れたスタブ」ではなく意図した設計。
- `open-web-server-gateway` に OpenTelemetry 連携(`src/telemetry.rs`)を追加済み
  (2026-07-11)。`grant_item`/`charge` ハンドラがスパン化され、
  `OTEL_EXPORTER_OTLP_ENDPOINT` の有無で OTLP/HTTP エクスポートと標準出力
  フォールバックを切り替える。テストはインメモリエクスポータで検証。

## 運用ルール追記(2026-07-18、正本はopen-raid-zのCLAUDE.md参照) — 確認不要の自動継続・リミット解除後の自動再開

- **コンテキストウインドウ・5時間利用制限・その他のセッション中断が
  発生し、その後リミットが解除されて新しいセッションが開始された場合、
  「続けてよろしいですか」等の確認を挟まず、毎回自動的に前回セッションの
  続きの作業を再開すること**(ユーザー指示、2026-07-18)。具体的には:
  1. セッション開始時、各リポジトリの`git status`/`git log`と、この
     `CLAUDE.md`(および他プロジェクトのCLAUDE.md)のHANDOFF節・
     「次にすべきこと」記載を確認し、未完了・未pushの作業が無いかを
     まず裏取りする(タスク管理メタデータを鵜呑みにしない既存方針と
     同じ姿勢で、実際のgit状態を確認する)。
  2. 未完了作業が見つかった場合、ユーザーへの確認を求めず、そのまま
     自動的に検証(build/test)→修正→コミット→pushまで完了させる。
  3. 完了している場合は、各CLAUDE.mdの「次にすべきこと」「未着手・
     未完成」に記載された次の項目へ確認なしに着手する(既存の
     「未着手だからといって確認を求めて手を止めない」方針の延長)。
  4. 「続けてよろしければそのまま自動開発を継続します」のような、
     続行そのものを尋ねる確認は今後一切行わない(ユーザー指示、
     2026-07-18)。作業内容の要約・進捗報告はしてよいが、それは
     承認を求めるものではなく完了報告として書く。
  5. こまめにコミット・pushしておくことで、次回セッションが「どこから
     再開すべきか」を迷わず`git log`/CLAUDE.mdから機械的に判断できる
     ようにしておく(区切りがついた時点で都度コミット・pushする既存
     方針との組み合わせ)。


## 運用ルール追記(2026-07-19、正本はopen-raid-zのCLAUDE.md参照) — 白画面バグ等を見逃さない検証徹底

- **WEB/UIを持つ機能を実装した後は、ビルド成功・`cargo test`・curlでの
  ステータスコード確認だけで「完了」と報告せず、実際に画面が正しく
  表示される(白画面・レンダリング崩れ・コンソールエラーが無い)ところ
  まで確認すること**(ユーザー指示、2026-07-19)。
  1. ブラウザ操作が可能な環境では、実際にページを開いて表示内容
     (見出し・本文・想定した要素の存在)とコンソールエラーの有無を
     確認する。
  2. ブラウザ操作ができない環境では、少なくとも`curl`等でHTMLボディの
     中身を取得し、期待される文字列が実際に含まれているかを確認する
     ——ステータスコード200だけを見て「動作確認済み」としない。
  3. 白画面・エラー・期待した内容の欠落等の不具合が見つかった場合は、
     確認を求めず自動的に原因調査・修正・再確認まで行う。
  4. 本番ドメインが未取得・DNS未設定なだけの状態は上記の「白画面
     バグ」とは別物であり、混同しない(`localhost`確認で代替可)。


## HANDOFF (直近の自動巡回ログ、上が最新)

- **2026-07-19 `KeyGuardian`にファイルバックド永続化を追加
  — 前回HANDOFF(2026-07-18)で明記した「正直な開示: プロセス内メモリのみ、
  再起動で全キー消失」というギャップの解消**: このリポジトリの根本方針
  (24時間365日ノンストップ・「二重課金」も「データ消失」も起こさない
  ミッションクリティカル設計)に対し、自己運用型APIキーレジストリ自体が
  再起動で発行済みキー・失効状態を丸ごと失う設計は矛盾するとの指摘を
  受けて着手。新規DBエンジン・外部サービスは追加せず、既存のこの
  エコシステムの「軽量なローカル永続化」の作法(`audiocafe-tokyo-rust`の
  `cron.rs`が`*-cache.json`へ`serde_json::to_string_pretty`+
  `std::fs::write`で書く方式と同じ重さ感)に、キーデータの重要性に見合う
  **アトミック書き込み**(一時ファイルへ書いてから`rename`、クラッシュ
  時に半端なJSONで永続化ファイルが壊れることを防ぐ)を足したもの。
  - `crates/open-web-server-gateway/src/keyring.rs`: `GuardianConfig`に
    `persistence_path: Option<PathBuf>`を追加(`from_env()`が新規の
    `OPEN_WEB_SERVER_KEY_STORE_PATH`環境変数から読む。未設定時は既存の
    プロセス内メモリのみの挙動を一切変えない、既存デプロイ・既存テスト
    への影響ゼロを優先)。`issue`/`revoke_owner`/`verify`内の期限切れ
    自動クリーンの3箇所すべてで、レコードの`Mutex`ガードを保持したまま
    (`persist_locked`)ハッシュ済みキーのみのJSONをアトミック書き込み
    する——ロックを離してから書くと2スレッドの書き込み順序が入れ替わり
    「後勝ちだが古い内容」で上書きされるレースになり得るため、ロック
    保持中に直列化して書く設計とした(プレーンテキストキーは既存設計
    通り一切保存しない)。
  - `KeyGuardian::load_from_disk(config)`を新設(`KeyGuardian::new`は
    テスト・後方互換のため残す)。永続化ファイルが無い/読めない/JSON
    パース失敗のいずれでも`tracing::warn!`(または未作成時は`info!`)を
    出すのみで**パニックせず空のレジストリから起動する**——補助的な
    認証利便性機能であり本体の起動を止める理由にはしない、という
    既存のACME/監査ログ等と同じ「補助系の失敗は権威パスをブロック
    しない」設計方針を踏襲。`crates/open-web-server-gateway/src/
    state.rs`の`AppState::from_env()`を`KeyGuardian::new(...)`から
    `KeyGuardian::load_from_disk(...)`へ差し替え。
  - **検証**: 新規依存追加なし(`serde_json`は既存依存)。
    `cargo build -p open-web-server-gateway`成功。`cargo test -p
    open-web-server-gateway`は**34件全green**(前回29件から+5件が
    今回の永続化テスト): (1)キー発行→実一時ファイルパスへ永続化した
    独立した第二の`KeyGuardian`インスタンスが同じファイルから状態を
    復元し`verify()`が正しく認識する往復実証
    (`issued_key_survives_reload_into_a_fresh_instance`)、(2)失効も
    同様に永続化・再読込後に生存する実証
    (`revocation_survives_persist_and_reload`)、(3)永続化ファイルが
    存在しない場合はパニックせず空レジストリで起動
    (`missing_persistence_file_starts_empty_without_crashing`)、
    (4)非JSONの壊れたファイルでもパニックせず空レジストリで起動
    (`corrupted_persistence_file_starts_empty_without_crashing`)、
    (5)`verify()`内の期限切れ自動クリーンによる削除もディスクへ
    反映される実証(`expiry_cleanup_persists_removal`)。既存の
    `keyring`単体テスト6件・`handlers::keys`のテスト・実HTTP経由の
    統合テスト`keyguardian_issued_key_authorizes_admin_requests_over_
    real_http`を含む既存29件は無変更のまま全てgreen。
    `cargo test --workspace`も全クレートでリグレッション無し(gateway
    34件・ledger 20件(1件ignored)・wire 18件、他クレートは既存通り)。
  - **正直な開示・次にすべきこと**: (1) `open-web-server-ledger`との
    本格統合(WAL/監査ログと同じ永続化層への統合)は今回のスコープ外
    (今回はシンプルなJSONファイル1本に留めた、前回HANDOFFが明記した
    次段階課題そのものへの対応としては最小実装)。(2) 永続化ファイルの
    ローテーション・サイズ上限は未実装(キー件数が非常に多い運用では
    将来検討)。(3) マルチインスタンス(複数プロセス)間での永続化
    ファイル共有・ロック競合(NFS等での同時書き込み)は未検証——単一
    プロセスでの再起動耐性のみを今回のスコープとした。

- **2026-07-18 `KeyGuardian`(自己運用型APIキーレジストリ)を新規実装
  — ユーザー指示「第二のTomcatでREST API不要でAPIキーの自動発行・
  自動承認・自動廃棄・APIキーを意識しない仕様、WunderGraph Cosmo有料版
  互換性向上」**: `RPoem`/`RCosmo`の`crates/open-runo-router/src/
  keyring.rs`と同じ設計(auto-issue・auto-revoke・期限切れの
  auto-clean・EWMAによる異常検知の自動防衛)を、
  `crates/open-web-server-gateway/src/keyring.rs`として自己完結で
  再実装した(**RPoem側の`open_runo_db::DbBackend`には依存しない**
  ——別リポジトリのcrateへ直接依存させない既存方針を守るため、
  ロジックのみを移植し、永続化は本リポジトリ独自のプロセス内メモリ
  実装とした)。
  - `handlers::keys`: `POST /admin/keys`(自動発行)・
    `POST /admin/keys/revoke`(owner名義の全キーを自動失効)・
    `GET /admin/keys`(発行済みキー件数のみ、プレーンテキストは
    再表示しない設計)。
  - `handlers::tenants::check_admin_auth`を拡張し、既存の静的共有
    シークレット(`x-admin-token`)**だけでなく**、`KeyGuardian`が
    発行した`Authorization: Bearer <key>`でも管理APIを通せるように
    した(**「APIキーを意識しない」の核心**: 一度キーを発行すれば、
    以後の呼び出し元は共有シークレットの存在自体を知らなくてよい)。
    最初の1本を発行する行為だけは、既存の静的シークレットを持つ人が
    行うブートストラップ設計(`handlers/keys.rs`のdoc comment参照)。
    `handlers::tls`の3エンドポイントも含め、既存の全管理APIへ
    後方互換を保ったまま反映(引数追加のみ、静的シークレット単体でも
    引き続き動作する)。
  - **検証**: `cargo test -p open-web-server-gateway`で**29件全green**
    (新規8件: `keyring`モジュール単体6件+`handlers::keys`のヘッダ解析
    単体1件+実HTTP経由のエンドツーエンド統合テスト1件)。統合テストは
    実際に(1)静的シークレットで`POST /admin/keys`を叩いてキーを自動
    発行→(2)発行された動的キーのみ(静的シークレットは一切送らず)で
    `GET /admin/tenants`が通ること→(3)`POST /admin/keys/revoke`で
    自動失効させた後は同じキーがもう通らないこと、を実際のTCP接続・
    HTTPリクエストで確認した(型チェックのみでの完了報告ではない)。
    `cargo test --workspace`もリグレッション無し。
  - **正直な開示・次にすべきこと**: (1) 現状はプロセス内メモリのみ
    (再起動で全キー消失)。`open-web-server-ledger`との統合による
    永続化が次段階の課題(RPoem側はPostgreSQL永続化まで実装済み)。
    (2) SCIMプロビジョニング連動の自動発行(RPoem側は`scim_create_user_
    handler`等と連動済み)は本リポジトリにSCIM自体が無いため範囲外
    のまま。(3) 発行済みキーの一覧・owner別内訳等の可視化APIは
    件数のみ(`GET /admin/keys`)に留まり、詳細一覧は未実装。

- **2026-07-17 ACMEクライアント本体(Phase 2)を`poem-cosmo-tauri`から
  移植完了 — ユーザー指示「123の順で」の1番目**: 前回HANDOFF(Phase 1)
  で「型が深く結合しており1パスでは移植しきれない」としていた
  ACMEクライアント本体(ディレクトリ探索・nonce管理・JWS署名・
  account/order/challenge/finalizeステートマシン)を、実際には
  `open_runo_core::{AppError, Result}` → `anyhow::Result`という型の
  違いを機械的に置き換えるだけで移植できることが分かり、完了させた
  (JWS/JWK/base64url/CSR構築のロジックは無変更)。
  `open-web-server-gateway`に`acme` Cargo feature(既定オフ、
  `reqwest`/`ring`をoptional依存化)を新設し、その配下でのみ
  コンパイル。`obtain_certificate_http01()`+管理API
  `POST /admin/tenants/:host/tls/acme`
  (`{"directory_url","contact_email"}`、成功時は自動で
  `TenantCertResolver::upsert_pem`へ登録)を追加。
  **検証**: `cargo test -p open-web-server-gateway --features acme`
  (27件、新規4件+モックCAエンドツーエンドテスト1件)・
  `cargo test --workspace --features open-web-server-gateway/acme`
  ともgreen。特にエンドツーエンドテストは、本物の
  `challenge_response_handler`と実TCP上のモックACME CAを組み合わせ、
  モックCAが**本当にループバックHTTP経由でこのプロセスの
  `.well-known/acme-challenge`へGETしてkey authorizationを確認する**
  ことで、discover→account→order→challenge公開→検証→finalize→
  ダウンロードの一気通貫を実証(JWS署名自体の暗号検証はモックCA側では
  行わない——それにはこのテストが検証したいクライアント側ロジックを
  サーバー側で再実装する必要があるため)。詳細は`docs/tls-tenant.md`。
  **正直な限界**: 実Let's Encrypt(staging/production)への実接続は
  未検証——公開ドメイン・ポート80への外部到達性が必要なため、次の
  優先項目「実VPS・実ドメインでの動作検証」で扱う。

- **2026-07-16(続き) ACME HTTP-01チャレンジレスポンダ(Phase 1)を追加
  — 前回HANDOFFの「次回フェーズ候補」を一部解消**: 新規
  `crates/open-web-server-gateway/src/acme.rs`——`ChallengeStore`
  (トークン→key-authorizationのインメモリ対応表)+
  `GET /.well-known/acme-challenge/:token`ハンドラ。暗号/HTTP
  クライアント依存が無いため常時コンパイル、`AppState.acme_challenges`
  として配線済み。ACME CA(Let's Encrypt等)や外部ACMEクライアント
  (certbot等)がこのプロセスに向けて発行したチャレンジをそのまま
  配信できる。
  **意図的にPhase 2(ACMEクライアント本体)は今回移植しなかった**:
  `poem-cosmo-tauri`側の手書きACMEクライアント(HTTP-01/DNS-01/
  TLS-ALPN-01、`open-runo-router/src/acme.rs`の
  `#[cfg(feature = "acme")] mod client`、~1500行)は
  `open_runo_core::{AppError, Result}`・`crate::hyper_compat::
  {Handler, Params}`というpoem-cosmo-tauri固有の型に深く結合しており、
  このリポジトリの型体系(`response::BoxBody`等)へ1パスで安全に移植
  しきれる規模ではないと判断——型を1つずつ対応させながら次回セッション
  で移植することを推奨する(詳細・判断根拠は`docs/tls-tenant.md`参照)。
  **検証**: `cargo test -p open-web-server-gateway`(21件、新規
  `acme::tests`2件含む)・`cargo test --workspace`(全クレート)
  ともgreen(WSL Ubuntu、rustc/cargo 1.97)。

- **2026-07-16 テナント別TLS終端(Phase 1) — open-web-server自体を
  Apache+Nginxハイブリッド相当に近づける最初の実装、WSL Ubuntu
  (rustc/cargo 1.97)で実TLSハンドシェイクまで検証済み**: ユーザーから
  「ApacheやTomcatの代わりになるWEBミドルウェア・フレームワークとして、
  ZFS互換・ACID互換のハイブリッドなど、まだ未完成の関連リポジトリの
  完成度と実用性を高めてよい」との指示を受け、`open-web-server`の
  既知の欠落(TLS終端を実nginx/certbot経由の外部プロセスに依存しており、
  自己完結したApache+Nginx代替になっていない)から着手。
  **設計判断の根拠(2026-07-16、EN/JP両言語でGoogle検索+GitHub調査
  済み)**: `rustls::server::ResolvesServerCert` + ホスト名ごとの
  `CertifiedKey`辞書というSNIベースの証明書切替パターンは、実世界の
  同種実装(複数ドメインをTLS終端するRust製リバースプロキシ`rpxy`等)
  でも使われている標準的な設計であることを確認済み。ACME自動取得に
  関しては、2026年時点で`instant-acme`(アクティブにメンテナンスされた
  pure-Rust実装、レート制限・アカウントキャッシュ等の実務上の懸念に
  対応済み)が本番運用の推奨選択肢だが、`poem-cosmo-tauri`側に既に
  実装・テスト済みの手書きACMEクライアント(HTTP-01/DNS-01/TLS-ALPN-01)
  があり、新規依存を追加しないという既存方針とも一致するため、
  今回は証明書の**手動/API登録**までを実装し、ACME自動化は
  `poem-cosmo-tauri`側実装の移植として次回フェーズに明記するに留めた
  (`docs/tls-tenant.md`に判断根拠を記載)。
  **実装**: (1) `open-web-server-wire::TenantCertResolver`
  (`crates/open-web-server-wire/src/tls.rs`)——`ResolvesServerCert`
  実装、`upsert_pem`/`upsert_from_files`/`remove`/`contains`。
  (2) `build_tenant_server_config(resolver)`——このリゾルバを使う
  `rustls::ServerConfig`を組み立てる。(3)
  `open-web-server-gateway`の`main.rs`に`accept_tls_loop`(既存の
  プレーンHTTP`accept_loop`とルーティングロジックを完全共有、違いは
  ハンドシェイク層のみ)、`OPEN_WEB_SERVER_TLS_BIND`環境変数で
  有効化(未設定時は従来通りプレーンHTTPのみ、既存動作を壊さない)。
  (4) 管理API`POST`/`DELETE /admin/tenants/:host/tls`
  (`handlers/tls.rs`、既存の`OPEN_WEB_SERVER_ADMIN_TOKEN`認証を再利用、
  証明書登録とHTTPルーティング登録は意図的に独立操作)。
  **検証(実TLSハンドシェイク、新規テストテナントのみ・本番nginx
  設定は一切変更していない)**: `cargo test -p open-web-server-wire
  tls::`(4件)——`rcgen`の使い捨て自己署名証明書2組を使い、同一
  `ServerConfig`が2つの異なるSNI名に対して実際に異なる証明書を返す
  ことを実TCPループバック上のTLS 1.3ハンドシェイクで証明
  (`real_tls_handshake_resolves_different_cert_per_sni`)。
  `cargo test -p open-web-server-gateway
  tests::tls_admin_registration_enables_real_tls_handshake_and_dispatch`
  ——証明書登録→`accept_tls_loop`が実際にそのSNI名向けTLS
  ハンドシェイクに成功→TLS越しの`GET /healthz`が実際に`dispatch()`
  まで届き200を返す、というエンドツーエンドの経路を実TCP上で証明。
  `cargo test --workspace`(全クレート)も実行し既存テストへの
  リグレッションが無いことを確認(結果はこのエントリ更新直後の
  コミットログ参照)。
  **cargo実行環境について**: このセッションからはWSL Ubuntu
  (`wsl -d Ubuntu`、rustc/cargo 1.97)経由でビルド・テストを実行できる
  ことが判明(詳細はopen-runo側CLAUDE.mdの同日「運用ルール」節参照)
  ——これにより過去のHANDOFFで繰り返し記録されていた「sandboxの
  cargo 1.75では一部依存がedition2024を要求しビルドできない」という
  制約を回避できるようになった。
  **次回フェーズ候補**: (1) ACME自動取得(`poem-cosmo-tauri`の
  `acme.rs`移植)、(2) `accept_tls_loop`のHTTP/2・WebSocketアップグレード
  対応、(3) `tenant_router::TenantConfig`とTLS証明書登録の統合
  (現状は`/admin/tenants`と`/admin/tenants/:host/tls`が別API)。
  詳細は`docs/tls-tenant.md`を参照。

- **2026-07-15 コードヘルス監査 — audit only, no changes**:
  `cargo build --workspace`/`cargo test --workspace`を実行し、ビルド成功
  (警告1件: `tenant_router.rs`の`len`/`is_empty`が未使用、実害なしの
  dead_code警告)・全51テストgreen(1件ignored)を確認。以前のHANDOFF
  エントリに記録されていた「cargo 1.75 + edition2024でworkspace全体の
  `cargo check`が実行できない」という環境制約は、現在の環境では
  再現しなかった(問題なくビルド・テストできた——ツールチェーンが
  更新された可能性)。`git status`はクリーン、修正すべき壊れたビルド・
  失敗テスト・小規模な欠落は見つからなかったため、コード変更は
  行っていない。

- **2026-07-14(続き) 前回HANDOFFの未決事項「poem-cosmo-tauri側への
  ミラー要否」を調査・決着 — このリポジトリ側のコード変更は不要と判明**:
  前回、`GET /internal/db/state/...`プロキシがopen-runo固有のまま
  だった件について「poem-cosmo-tauriのスコープに合うか要検討」と
  未決のまま残していた。調査の結果、根本原因は**poem-cosmo-tauri側に
  `GET /api/db/:table/:key/at/:commit_id`自体が実装されていなかった
  こと**であり(該当パスはpoem-cosmo-tauri側のCLAUDE.md参照——同日中に
  修正済み。ついでにAruaruDbBackendが実aruaru-serverに対して一度も
  動作しない実バグを抱えていたことも判明・修正された)、
  **このリポジトリの`DbStateReader`/`GET /internal/db/state/...`
  ハンドラは`OPEN_RUNO_ENDPOINT`環境変数だけでopen-runo/
  poem-cosmo-tauriどちらの実装も指せる設計になっている**ため、
  こちら側に追加のコード変更は一切不要と判断。
  **実証**: 実バイナリ3つ(`open-runo-router`・`poem-cosmo-tauri`版
  `open-runo-router`・`open-web-server`)を用意し、`open-web-server`を
  `OPEN_RUNO_ENDPOINT=http://127.0.0.1:<poem-cosmo-tauriのポート>`で
  起動、コード変更なしに同じ`GET /internal/db/state/game_items/
  player-1/at/some-commit`が正しく`502`
  (`"...returned unexpected status 501 Not Implemented..."`)を返す
  ことを確認——open-runo版に対する検証(前回HANDOFFエントリ)と
  バイト単位で同一の結果。
  次回パスがすべきこと: 特に緊急の課題は無い。残る既知ギャップは
  (1)レートリミット/セッション状態のマルチインスタンス間共有、
  (2)`aruaru-wire`の拡張プロトコル(prepared statement)非対応
  (多くのORM/ドライバのデフォルト経路が失敗する実用性ギャップ、
  aruaru-db側のスコープ)。

- **2026-07-14 拡張要件(1)「VersionLessAPI + Git管理ハイブリッド」の
  読み出し側をopen-web-server側にも実装 — これで書き込み側・読み出し側
  両方が揃った**: open-runo側は2026-07-13に`GET /api/db/:table/:key/
  at/:commit_id`を実装・検証済みだったが、`open-web-server`側に対応する
  エンドポイントが無く「未接続」のまま残っていた(前回HANDOFFで
  明記されていたギャップ)。ユーザー指示(ハイスピード・ハイセキュリティ・
  4層4重の通信/DB連携の実用性向上)を受けて着手。
  **新規実装**: (1)`open-web-server-core::DbStateAtCommitResponse`
  (`target`/`key`/`commit_id`/`value`、`MutationRequest.target`と同じ
  target空間を使う)。(2)`open-web-server-ledger::DbStateReader`
  (`Ledger`の書き込みパス`forward_once`とは独立したHTTPクライアント。
  open-runoの`GET /api/db/:table/:key/at/:commit_id`へプロキシ。
  **認証は`POST /api/keys/self-issue`による自動キー発行+キャッシュ+
  `401`時の透過的再発行**——`open-runo-cli`/WASMフロントエンドと同じ
  「人間がAPIキーを意識しない」方針をここでも踏襲。404は`Ok(None)`
  (正常、エラーではない)として扱う)。(3)
  `open-web-server-gateway`に`GET /internal/db/state/:target/:key/
  at/:commit_id`ハンドラ(`handlers/state_query.rs`)——このゲートウェイの
  ディスパッチャは動的パスパラメータの無い素のmatch式のため、
  `path.starts_with(...)`ガード+手動パースで対応。open-runo側が
  想定外のステータス(バックエンド未対応の501含む)を返した場合は
  このゲートウェイの`502 Bad Gateway`として伝える(404=「このゲートウェイ
  自体にリソースが無い」とは区別)。
  **検証**: `DbStateReader`の単体テスト3本(実HTTPモックopen-runoサーバー
  相手に、既知target/key/commit→実際の値取得・未知commit→`Ok(None)`・
  self-issueキーが複数リクエストにまたがってキャッシュされ1回しか
  発行されないことを確認)、ハンドラのパス解析テスト4本、
  `cargo test --workspace`全体green。**さらに型チェックのみで終わらせず、
  実バイナリ2つ(`open-runo-router`+`open-web-server`)を実際に起動して
  検証**: `open-runo-router`(in-memoryバックエンド、コミット履歴クエリ
  自体に未対応)に対しself-issueキー取得→`open-web-server`をその
  `OPEN_RUNO_ENDPOINT`で起動→`GET /internal/db/state/game_items/
  player-1/at/some-commit`を叩き、open-runo側の実`501 Not Implemented`
  (`"AS OF COMMIT reads are not supported by the 'in-memory' backend"`)
  がこのゲートウェイの`502 Bad Gateway`として正しく伝播することを実HTTP
  で確認(検証中、最初に誤ってpoem-cosmo-tauri側の`open-runo-router`
  バイナリを起動してしまい——このエンドポイントはopen-runo固有で
  poem-cosmo-tauriにはまだミラーされていない——`404`のみ返る実バグ
  ではない誤操作に気づき、正しいopen-runo側バイナリで再検証した)。
  `docs/integration.md`(古い「Rust + Poem」スタック表記も合わせて修正)
  を更新。
  次回パスがすべきこと: (1)このエンドポイントをpoem-cosmo-tauri側にも
  ミラーするか判断(現状open-runo固有、`aruaru-db`バックエンド連携が
  前提の機能のため、poem-cosmo-tauriのスコープに合うか要検討)、
  (2)レートリミット/セッション状態のマルチインスタンス間共有
  (open-runo/poem-cosmo-tauri側の既知ギャップ、Redis等への移行)、
  (3)`app_proxy`と本エンドポイントの責務分離の見直し(現状は別々の
  ハンドラだが、将来的に統一的なプロキシ層にまとめる余地がある)。

- **2026-07-14(Apache+Tomcat型のWebサーバー/アプリケーションサーバー連携を追加、
  ユーザー指示)**: `open-web-server-gateway`が単体動作を保ったまま、設定時のみ
  アプリケーションサーバー層(`open-runo`/`poem-cosmo-tauri`)へ処理を委譲できる
  ようにした。新規 `crates/open-web-server-gateway/src/app_proxy.rs`:
  `OPEN_WEB_SERVER_APP_UPSTREAM` 環境変数(例: `http://127.0.0.1:8080`)が
  設定されている場合のみ、`main.rs`の`dispatch()`で既存ハンドラ
  (`grant_item`/`charge`/`healthz`)のどれにも一致しなかったリクエストを
  `hyper_util::client::legacy::Client`でそのままHTTP転送し、応答をそのまま
  返す(メソッド・パス・クエリ・ヘッダ・ボディを保持、到達不能なら
  `502 Bad Gateway`)。環境変数が未設定なら従来通り`404`(=完全に単体動作、
  Tomcatが無くてもApacheが動くのと同じ関係)。
  **汎用性の設計判断(ユーザー指示、2026-07-14)**: 転送先はプレーンHTTPの
  ため、Rust製の`open-runo`/`poem-cosmo-tauri`に限らず、PHP-FPM/Python
  (ASGI)/Ruby(Puma/Unicorn)/Perl(PSGI/Plack)等、HTTPで応答する任意の
  言語・フレームワークのアプリケーションサーバーを同じ仕組みで指せる
  (`open-easyweb`の`gen-vhost.sh --stack=proxy`のUPSTREAMと同じ考え方)。
  **検証**: `cargo build -p open-web-server-gateway`成功、
  `cargo test -p open-web-server-gateway`既存4件すべてgreen(新規の
  結合テストは未追加——実際のapp-server起動を伴う統合テストは次回パスで
  追加予定、正直な限界として明記)。
  **open-easyweb側との連携**: `open-easyweb`の`SiteProfile`に
  `app_server`("none"/"open-runo"/"poem-cosmo-tauri")・
  `app_server_upstream`(host:port)フィールドを追加し、ドメインごとに
  アプリケーションサーバーを選択・登録・変更・削除できるUIを追加。
  新規`scripts/switch-app-server.sh`で、デプロイ済みvhostの転送先を
  後から書き換え可能(詳細はopen-easyweb側CLAUDE.md参照)。
  **未着手として明記**: 実際に`open-runo`/`poem-cosmo-tauri`インスタンスを
  起動した状態でのエンドツーエンド転送検証(実バイナリ2つを同時起動して
  `curl`で確認)は次回パスの課題。

- **2026-07-14 (続き) マルチテナントルーターの1→3完了(リバースプロキシ転送・
ルーティング配線・管理API認証)**:
- `proxy::forward()` 新設: `hyper_util::client::legacy::Client`
  (プロセス全体で1つを`OnceLock`共有、ドメインが増えてもクライアント自体は
  増やさない)で`tenant.backend_addr`へリクエストを中継し、応答をそのまま
  返す。到達不能時は`502 Bad Gateway`。
- `main::dispatch()`にHostヘッダからの`tenants.resolve()`を配線: 既存の
  `/api/v1/*`・`/admin/*`・`/healthz`のいずれにも一致しないリクエストを
  マルチテナントルーティング対象とし、該当ドメイン登録があれば
  `proxy::forward()`へ、無ければ`404`。
- `handlers::tenants`に`OPEN_WEB_SERVER_ADMIN_TOKEN`環境変数による簡易
  共有シークレット認証(`x-admin-token`ヘッダ比較)を追加。未設定時は
  無検証(開発用途)。本番運用ではmTLS/OAuth等への置き換えを推奨、と
  コード内docに明記。
**検証**: 独立クレート(`tokio`/`hyper`/`hyper-util`のみ)で実TCPループバック
上のE2Eテストを実施——疑似バックエンドを1つ起動し、`forward()`を挟んだ
"ゲートウェイ役"リスナーへ実際にHTTPリクエストを送り、ステータス・
ヘッダ・ボディがバックエンドの応答と一致することを確認(green)。
`tenant_router`単体テスト9件も引き続きgreen。workspace全体の実バイナリ
起動確認は、前回記載の環境制約(cargo 1.75 + edition2024)により未実施。
**未着手(次回)**: (1) `backend_addr`が到達不能な場合のリトライ/
サーキットブレーカー、(2) HTTP/2・WebSocketアップグレードの中継対応
(現状HTTP/1.1のみ)、(3) 管理APIの認証をトークン比較からmTLS/OAuthへ。

**2026-07-14 マルチテナント・ドメインルーター(open-easyweb構想)第一実装**:
ユーザーからの提案「ドメイン/サブドメインごとにWebサーバー・バックエンド
(open-runo/poem-cosmo-tauri)・DBを個別インストールするのは面倒 →
マルチコア・マルチスレッド・非同期の『分身の術』でもっと賢く」を受けて、
`open-web-server-gateway`に以下を新設。
- `tenant_router::TenantRegistry`(`tokio::sync::RwLock<HashMap<host, TenantHandle>>`):
  ドメイン追加/削除がプロセス再起動・ポート個別割り当てを伴わない設計。
  「分身の術」はOSプロセス/スレッドの複製ではなく、tokioの
  マルチコアワークスティーリングランタイム上で自然に分散される軽量
  非同期タスク単位の複製として実現(新規プロセスは一切増やさない)。
- `load_from_toml()`: `domains.toml`(例: `domains.toml.example`)1本からの
  一括宣言的プロビジョニング。個別インストール手順の代替。
- `handlers::tenants`: `POST /admin/tenants`(追加)・
  `DELETE /admin/tenants/:host`(削除)・`GET /admin/tenants`(一覧)。
  無停止での動的追加/削除を管理APIとして配線。
- `AppState::load_domains_from_env()`: `OPEN_WEB_SERVER_DOMAINS_FILE`環境変数
  が設定されていれば起動時に一括ロード。
**検証**: このサンドボックスの`open-web-server`ワークスペース全体は
既知の環境制約(cargo 1.75では`quinn`/`opentelemetry`等の依存が要求する
edition2024が原因でロックファイル自体を解決できない、変更前から発生する
既存の問題であることを`git stash`比較で確認済み)により`cargo check`が
実行できない。そのため`tenant_router.rs`のロジックを標準ライブラリ+
`tokio`/`serde`/`toml 0.7`/`thiserror`のみの独立クレートへコピーして検証し、
`cargo test`で9件全てgreenを確認(追加/検索/重複拒否/削除/削除後の解決不能/
upsert/TOML一括ロード/一覧取得)。本体クレートへの統合(`toml`は0.7へpin、
`state.rs`/`main.rs`への配線)はコード上完了しているが、workspace全体の
実バイナリ起動検証は未実施(環境制約により次回、cargo 1.85以降の環境で
要実施)。
**未着手(次回以降)**: (1) 実際のリバースプロキシ処理(現状は
`TenantHandle`の検索までで、`backend_addr`へのHTTP転送は未実装)、
(2) 管理API `/admin/tenants` の認証・監査ログ、(3) Host検索結果を
`dispatch()`のルーティングに実際に組み込む配線(現状`tenant_router`は
独立モジュールとして追加されたのみで、通常リクエストパスとの接続は次回)。

- **2026-07-14 マルチテナント・ドメインルーター(`tenant_router`)を
  `app_proxy`と統廃合・融合(ユーザー指示「良い所取りで統廃合して融合して」)**:
  同日、別セッションが上記の`app_proxy.rs`(単一アップストリームへの委譲、
  `OPEN_WEB_SERVER_APP_UPSTREAM`)を実装済みだったところに、こちらの
  セッションが独立に「ドメイン/サブドメインごとに複数バックエンドを
  動的振り分けするマルチテナントルーター」(`tenant_router::TenantRegistry`)
  を実装しており、機能が重複していた。ユーザー確認の上、両者を統合:
  - `tenant_router::TenantRegistry`(`tokio::sync::RwLock<HashMap<host,
    TenantHandle>>`): ドメイン追加/削除がプロセス再起動・ポート個別割り当てを
    伴わない設計。「分身の術」はOSプロセス/スレッドの複製ではなく、tokioの
    マルチコアワークスティーリングランタイム上で自然に分散される軽量
    非同期タスク単位の複製として実現。
  - `load_from_toml()`: `domains.toml`(例: `domains.toml.example`)1本からの
    一括宣言的プロビジョニング。
  - `handlers::tenants`: `POST /admin/tenants`・`DELETE /admin/tenants/:host`・
    `GET /admin/tenants`。`OPEN_WEB_SERVER_ADMIN_TOKEN`環境変数による
    簡易共有シークレット認証(`x-admin-token`ヘッダ、未設定時は無検証)。
  - **統合方針**: `proxy.rs`にプロセス全体で共有する`Client`
    (`OnceLock`)と、汎用転送関数`forward_to(base_url, req)`を集約。
    `app_proxy.rs`は独自にクライアントを都度生成していたのをやめ、
    `proxy::forward_to()`を呼ぶだけの薄いラッパーに変更(コード重複解消)。
  - **`dispatch()`のフォールバック優先順位**(重複を解消しつつ両方の
    ユースケースを維持): ①既存ハンドラ(`/api/v1/*`・`/admin/*`・
    `/healthz`)、②Hostヘッダでの`tenant_router`解決(マルチドメイン、
    複数バックエンドを同時に運用したい場合)、③`OPEN_WEB_SERVER_APP_UPSTREAM`
    (単一バックエンドのみで運用するシンプルな場合、Apache+Tomcat型の
    後方互換フォールバック)、④どれにも一致しなければ`404`。
  - これにより「1ドメイン1バックエンドの単純運用(`app_proxy`由来)」と
    「複数ドメイン/サブドメインを1プロセスで動的に振り分ける運用
    (`tenant_router`由来)」を、クライアント実装を重複させずに両立させた。
  **検証**: 独立クレート(`tokio`/`hyper`/`hyper-util`のみ)で実TCPループバック
  上のE2Eテストを実施——疑似バックエンドを1つ起動し、`forward_to()`を挟んだ
  "ゲートウェイ役"リスナーへ実際にHTTPリクエストを送り、ステータス・
  ヘッダ・ボディがバックエンドの応答と一致することを確認(green)。
  `tenant_router`単体テスト9件も引き続きgreen。workspace全体の実バイナリ
  起動確認は、環境制約(cargo 1.75 + edition2024)により未実施。
  **未着手(次回)**: (1) `backend_addr`が到達不能な場合のリトライ/
  サーキットブレーカー、(2) HTTP/2・WebSocketアップグレードの中継対応
  (現状HTTP/1.1のみ)、(3) 管理APIの認証をトークン比較からmTLS/OAuthへ、
  (4) `open-easyweb`の`SiteProfile`/`gen-vhost.sh`側からも`tenant_router`の
  管理APIを直接叩けるようにする連携(現状`open-easyweb`はNginx/Apache vhost
  生成のみで、本リポジトリのマルチテナントAPIの存在を前提にしていない)。

- **2026-07-13(実ドメインでのTLS/Let's Encrypt検証完了、ユーザー保有の
  runo.tokyoドメイン使用)**: ユーザーが実際に取得済みのドメイン
  `runo.tokyo`と実VPS(ConoHa、既に`aruaru`/`aruaru-easyweb`/nginx/
  PostgreSQL稼働中)を使い、`open-easyweb`(第二のKUSANAGI)経由での
  TLS自動化フローを実ドメインで検証した。DNS Aレコード設定→
  `certbot certonly --webroot`での実証明書取得→443番vhost新設→
  実インターネット経由での`curl`(証明書検証あり)による疎通確認、
  まで完了。過程で実バグ1件発見・修正(ACME webrootが`/root`配下に
  あり nginx ユーザーがトラバース不可、403エラーの原因)。
  詳細・出典は`open-easyweb`側の同日付CLAUDE.md HANDOFFを参照。
  これで拡張要件のTLS/HTTPS自動化に関する「実ドメインが無いため未検証」
  という制約は解消された。

- **2026-07-13(長年の懸案だったライブPostgreSQL検証を実施、成功)**:
  `cargo test --workspace`を再実行し、まず`open-web-server-wire`(14件、
  QUIC/aggligator-MPTCP代替含む)・`open-web-server-ledger`(17件)
  すべてgreenであることを再確認(前回パスの「完了」報告は虚偽ではなく、
  実際にgreenだった)。その上で、長らく「到達可能なライブPostgreSQLが
  無い」と記載してきた`postgres_wal::tests::
  live_postgres_append_and_commit_round_trip`(`#[ignore]`)を、
  WSL2 Ubuntu上に`apt-get install -y postgresql`(rootユーザーで実行、
  対話的sudoパスワード入力は要求されずインストール可能だった)で実際に
  導入し、`listen_addresses='*'`+`pg_hba.conf`にscram-sha-256行を追加、
  `DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/
  open_web_server_test`でWindows側から`cargo test -p open-web-server-ledger
  -- --ignored`を実行し**実際に成功させた**(`PostgresWal::connect`→
  `ensure_schema`→`append`→`mark_committed`→`is_already_processed`の
  実往復)。
  **ハマった点(次回パスへの申し送り)**: 初回・2回目の実行は
  「pool timed out while waiting for an open connection」で失敗した。
  原因はコードのバグではなく、**WSL2の軽量VMが、どのwsl.exeプロセスも
  アタッチしていない間はアイドルタイムアウトで停止・再起動を繰り返す**
  ため、Windows側からのTCP接続(`localhost`フォワーディング)がVMの
  起動・終了のタイミングと競合して間欠的に失敗していたこと。
  `wsl -d Ubuntu -u root -- bash -lc "sleep 300"`をバックグラウンドで
  起動してVMを起こしたままにしてから再実行したところ安定して成功した。
  今後この種のWSL2連携テストを行う際は、必ず長時間スリープのkeep-alive
  プロセスをバックグラウンドで起動してからWindows側のテストを実行する
  こと。**これで拡張要件(4)の①PostgreSQLも実接続検証済みとなり、
  ①②③④すべてが実装・検証済みとなった**(②aruaru-dbはaruaru-db側の
  スナップショット連携実装、③④は本リポジトリで前回実装・検証済み)。
- **2026-07-13(open-easyweb連携の実バイナリ検証、詳細はopen-easyweb側
  HANDOFF参照)**: `open-web-server-gateway`(バイナリ名`open-web-server`)
  を実起動し、`open-easyweb`の`scripts/gen-vhost.sh --stack=proxy`が
  生成するNginx/Apache vhostが実際に本サーバーの`/healthz`まで
  到達できることを、Windows側にwinget導入したnginx実プロセス経由の
  curl(200応答、両側のログで確認)、およびWSL2側Apacheの
  `apache2ctl configtest`(Syntax OK)で検証した。
- **2026-07-13 (拡張要件(4)③マルチリージョン同期レプリケーションの実装
  ——前回パスが未コミットのまま中断していた作業を検証・仕上げ)**:
  前回パス(バックグラウンドエージェント)が`crates/open-web-server-ledger/
  src/multi_region.rs`(352行、新規)と`Ledger`本体への結線(144行差分)を
  未コミットのまま残していた(セッション中断)。中身を検証したところ
  **実装は完成しており、実テストも全てgreenだった**ため、そのまま
  仕上げてコミット。
  **実装内容**: `MultiRegionReplicator`が、実SQLiteファイル2つ以上
  (`sqlx`の`sqlite`feature、新規workspace依存に追加)を「リージョン」の
  代替として使い、コミット時に**同期的に全リージョンへ書き込み、全員の
  ACKを待ってから成功を返す**(UDP冗長経路のfire-and-forgetとは対照的な
  設計)。障害ポリシーは明示的に選択可能: デフォルトは1リージョンでも
  失敗すればコミット全体を失敗させる厳格モード、`with_quorum(n)`で
  「M系統中N系統成功で可」というN-of-M縮退モードも用意。
  `CoreError::MultiRegionReplicationFailed`を新規エラー種別として追加。
  **検証**: `cargo test --workspace`で新規4テストを含む全17件green
  (1件は既存のPostgreSQL実接続要求テストで意図的にignored)——
  実ファイルベースのSQLiteリージョン2つに実際に書き込み、片方を意図的に
  失敗させた場合の厳格/縮退両モードの挙動、正常系での両リージョンへの
  反映、をすべて実I/Oで確認済み。これで拡張要件(4)の4系統中3系統
  (①PostgreSQL・②aruaru-db・③マルチリージョン同期レプリケーション)が
  実装済み、④独立監査ログも既存実装済みのため**拡張要件(4)は概念上
  4系統すべて揃った**(①のみ実PostgreSQL接続での検証は本サンドボックスに
  到達可能なインスタンスが無く未実施)。
  **未着手のまま残った項目(前回パスが着手予定だったが手つかず)**:
  拡張要件(1)の読み出し側クエリAPI(`commit_id`指定で過去状態を問い合わせる
  エンドポイント、`open-runo`側との連携が必要)——次回パスで着手。
  `cargo check --workspace`もgreen。
- **2026-07-13 (今回、拡張要件(4)④独立監査ログの実装 + 拡張要件(1)の
  現状調査、ユーザー指示——「未着手は次に着手すべき実装対象」ルールに
  従った継続実装)**:
  **調査(拡張要件(1) VersionLessAPI×Git版管理ハイブリッド)**:
  `open-web-server-ledger::Ledger::commit()`の現行実装を精査した。
  TCP経由の権威パス(`forward_once`)は`open-runo`のHTTPエンドポイントへ
  フォワードし、返ってきた`MutationReceipt.db_commit_id`(aruaru-dbが
  発行するGit-on-SQLコミットID)をそのままクライアントへの応答に含めて
  返す設計に**既になっている**——書き込み側は実質的にVersionLessAPI
  (エンドポイントはバージョン番号を一切含まない)とGit版管理
  (commit_idがレスポンスに乗る)のハイブリッドとして機能している。
  しかし「commit_idを指定して過去の状態を問い合わせる」読み出し側の
  クエリAPIは`open-web-server`側に一切存在しない(`open-web-server-gateway`
  のハンドラを確認したが、GET系の状態照会エンドポイント自体が無い)。
  これは`open-runo`側にaruaru-dbへの読み出しルートを新設する必要がある
  2リポジトリ以上にまたがる作業であり、1パスで安全に検証可能な単位に
  収まらないと判断し、今回は見送った(「未着手だから見送る」の単独判断を
  避けるため、まず現状を精査したうえでの判断であることをここに明記)。
  次回、`open-web-server-gateway`に`GET /internal/db/state/:target/at/
  :commit_id`相当のハンドラ第一実装(open-runo側の対応エンドポイントと
  セット)に着手する。
  **実装(拡張要件(4)④独立監査/突き合わせトランザクションログ)**:
  `crates/open-web-server-ledger/src/audit_log.rs`を新規作成。
  `FileAuditLog`(追記専用ファイル、各行`<sha256hex> <json>`形式で
  `AuditRecord`をチェックサム付き記録)、`scan_and_verify()`(全行の
  チェックサム再計算によるサイレント破損検出)、`reconcile()`
  (監査ログのidempotency_key集合とWAL側の確定済みキー集合を突き合わせ、
  「監査ログにあるがWAL未確定」「監査ログ内で同一キー重複」を検出——
  実際の金融機関の「主系とは別システムの冗長トランザクションログによる
  二重処理検知」パターンの最小実装)を実装。`Ledger`に
  `enable_audit_log(path)`(任意ビルダー)・`audit_log()`アクセサを追加し、
  `commit()`内でWAL先行書き込み直後に1レコード追記するよう配線(UDP冗長
  経路と同じ「補助系の失敗は権威パスをブロックしない」設計方針、書き込み
  失敗は警告ログのみ)。PostgreSQL/aruaru-db/マルチリージョン同期
  レプリケーションのいずれとも技術的に独立した実装(依存クレートも
  `sha2`のみ追加、他3系統とは無関係)。
  **テスト(実ファイルI/O)**: 単体テスト4件(round-trip・チェックサム
  破損検出・突き合わせレポート・存在しないファイルへのscan)を
  `audit_log.rs`に追加、加えて`Ledger::commit`経由の統合テスト1件
  (`commit_appends_a_verifiable_record_to_the_independent_audit_log`、
  実モックTCPサーバへの実コミット後、実ファイルへの追記・チェックサム
  検証・突き合わせレポートを一気通貫で検証)を`lib.rs`に追加。
  `cargo test -p open-web-server-ledger`は新規4件を含む全13件green、
  `cargo check --workspace`/`cargo test --workspace`も全31テスト
  (30 green + 1 ignored)を確認。
  **依存追加**: `sha2`(ワークスペースに既存)・`thiserror`
  (ワークスペースに既存)を`open-web-server-ledger/Cargo.toml`へ追加
  (新規外部クレートの追加は無し)。
  **未着手として明記**: 拡張要件(1)は上記調査の通り書き込み側は実質
  達成済みだが読み出しクエリAPIが未着手。拡張要件(4)の②aruaru-db・
  ③マルチリージョン同期レプリケーションは今回も未着手(理由: いずれも
  他リポジトリ(aruaru-db)または複数ノードの実環境構築を要し、本パス
  ではまず①②③④のうち最も本リポジトリ単体で完結する④を優先した)。
- **2026-07-13 (今回、WSL2実カーネルMPTCPの再調査——ユーザーが「本物の
  カーネル未サポートという結論が本当に行き止まりか」を懸念し再検証を
  明示指示。事前に「先行して裏で調査していたエージェントがいたはず」との
  申し送りがあったが、`git log`にWSL2/カーネルMPTCPを扱ったコミットは
  一切無く、`crates/open-web-server-wire/src/mptcp_channel.rs`にも
  該当する変更は無い——つまりそのような先行調査は実際には着手/着地して
  いなかった。よって本パスで最初からやり直した)**:
  **実施した確認(実コマンド出力)**: `wsl -d Ubuntu -- uname -r` →
  `6.18.33.2-microsoft-standard-WSL2`(MPTCP自体は上流Linux 5.6+で
  一般提供のはずのバージョン)。しかし `sysctl net.mptcp` →
  `cannot stat /proc/sys/net/mptcp: No such file or directory`、
  `ip mptcp limits` → `RTNETLINK answers: No such file or directory`、
  `/lib/modules/$(uname -r)/` 配下に`mptcp_diag`等のモジュール無し、
  `modprobe mptcp_diag` → `FATAL: Module mptcp_diag not found`。
  Microsoft製WSL2カーネルビルドは`CONFIG_MPTCP`を有効化していないと
  確定した(`/boot/config-*`自体も同梱されていないため設定ファイル上の
  直接確認は不可だが、上記の実行時証跡で機能欠如は明確)。
  **結論**: 以前(前回セッション)の「Windows開発環境ではカーネルMPTCP/
  SCTPに到達不能」という判断は、WSL2という代替経路を実際に試した上でも
  変わらず——本物の行き止まりであることを実コマンド証跡で確認した
  (以前は机上調査のみだった点が今回実証に格上げされた)。既存の
  `aggligator`ベースの代替実装(`mptcp_channel.rs`)はこの結論を踏まえた
  正しい判断だったとして維持し、コード変更は行っていない。
- **2026-07-13 (前回、拡張要件(3)④MPTCP/SCTPの調査と代替実装、
  ユーザー指示——四層四重アーキテクチャの継続実装。aruaru-db側の
  コミット×スナップショット連携第一段実装も並行して実施、詳細は
  aruaru-db側CLAUDE.md参照)**:
  **調査**: カーネルMPTCP/SCTPをこのWindows 11開発環境で実装・実ソケット
  検証することの実現可能性を先に確認した。(a) Windowsには`IPPROTO_MPTCP`
  相当のネイティブMPTCPソケットAPIが存在しない(Linuxカーネル5.6+限定の
  機能)。(b) 主要なRust SCTPクレート(`lksctp`/`sctp-rs`/`tokio-sctp`/
  `async-sctp`)はいずれも`lksctp-tools`というLinuxカーネルSCTPスタックへの
  バインディングであり、Windows版`sctp-sys`は「SctpDrv binding is
  experimental」と明記された実験的ドライバに依存する。(c) `sctp-proto`
  (純Rust Sans-IO実装)はOS非依存だが、相手ノードも同じ非標準実装を
  要求するため既存SCTPインフラとの相互運用性が無く、「SCTP実装」と
  称するにはミスリーディングと判断した。**結論: 本物のカーネルMPTCP/SCTP
  はこの開発環境では実装不可能な正直なブロッカー**。
  **代替実装**: `CLAUDE.md`運用ルール(未着手を確認だけして見送ることを
  禁じ、まず着手を試みることを求める)に従い、④の**目的**
  (物理経路マルチホーミングによる伝送路冗長化)を満たすユーザー空間の
  代替を調査し、`aggligator`/`aggligator-transport-tcp`クレート(公式docに
  "serves the same purpose as Multipath TCP and SCTP... completely
  implemented in user space without the need for any support from the
  operating system"と明記)を採用した。`crates/open-web-server-wire/src/
  mptcp_channel.rs`を新規作成し、`MptcpServer::bind_and_accept_one`
  (集約TCPサーバ、1接続=1メッセージの最小実装)・
  `send_mutation_over_mptcp`(クライアント側)を実装。ワイヤ形式は
  4バイトLE長プレフィクス+`MutationRequest`のJSON(①②③と同じ「1論理
  メッセージ往復」のスコープに揃えた)。**本物のカーネルMPTCP/SCTPでは
  ないことをモジュールdoc冒頭に明記**(偽って主張しない)。
  **テスト(実ソケット)**: `real_aggligator_roundtrip_over_loopback`
  (127.0.0.1のループバックで実TCPソケット上に実aggligator集約接続を張り、
  実データ往復を実証)。単一NIC環境(ループバック)のため、複数物理NICでの
  真のマルチホーミング効果自体はこのサンドボックスでは検証できない旨も
  正直に明記した。`cargo test -p open-web-server-wire`は新規1件を含む
  全14件green、`cargo check --workspace`/`cargo test --workspace`も
  全26テスト(25 green + 1 ignored)を確認。
  **依存追加**: `aggligator`0.9・`aggligator-transport-tcp`0.2
  (`default-features = false`、TLS機能は既存の4層防御通信と重複するため
  無効化)を`open-web-server-wire/Cargo.toml`へ追加。
  **並行実施(aruaru-db側)**: 拡張要件(2)の「次回新規開発予定」
  (aruaru-dbコミット×open-raid-zスナップショット連携)の第一段実装を
  aruaru-db側で実施——`aruaru-dist::raft::node::RaftNode`に
  `set_commit_hook`(commit+適用完了ごとに呼ばれるフック)を追加し、
  新設`aruaru-dist::snapshot_pairing`(`SnapshotBackend`トレイト・
  `SnapshotPairingRegistry`・`wire_to_node`配線関数)と、`open_raid_z`
  feature有効時のみコンパイルされる`aruaru-dist::raid_z_backend::
  OpenRaidZSnapshotBackend`(実際に`open_raid_z_core::pool::Pool`
  ・`create_snapshot`を呼ぶ)を実装。実Raft commit(`RaftNode::propose`→
  `try_commit_to`→`apply_committed`)が実RAID-Z2プール(6台の
  `FileBackedDevice`)上の実スナップショット作成をトリガーし、
  commit_index↔snapshot_idの対応関係を問い合わせられることを
  `real_raft_commit_triggers_real_raid_z_snapshot`統合テストで実証済み
  (詳細はaruaru-db側CLAUDE.md参照)。
  **未着手として明記**: 拡張要件(1)VersionLessAPI×Git管理のハイブリッド
  バージョン管理は今回も未着手。拡張要件(4)の②aruaru-db・③マルチ
  リージョン同期レプリケーション・④独立監査トランザクションログも未着手。
  aruaru-db側のコミット×スナップショット連携は「第一段」であり、
  永続化(プロセス再起動で対応表が失われる)・双方向リカバリは
  スコープ外として明記済み(aruaru-db側CLAUDE.md参照)。
- **2026-07-12 (前回、拡張要件(3)③QUICと拡張要件(4)①PostgreSQLの第一実装、
  ユーザー指示——四層四重アーキテクチャの継続実装)**:
  **① QUIC (`open-web-server-wire::quic_channel`)**: `quinn` 0.11
  (runtime-tokio + rustls features)を新規追加。`QuicServerConfig::self_signed`
  (開発/検証用の`rcgen`自己署名証明書生成、ALPN `open-web-server-quic` を
  サーバ/クライアント双方に設定)、`QuicServer::accept_one_mutation`
  (1接続=1双方向ストリームで`MutationRequest`をJSON受信しACKを書き返す)、
  `send_mutation_over_quic`(クライアント側、`insecure_client_config_trusting`
  で自己署名証明書のみ信頼)を実装。rustlsのCryptoProvider(ring)を
  `std::sync::Once`でプロセス内一度だけ明示インストールする必要があった
  (rustls 0.23の要件、複数バックエンドfeatureが有効な場合の曖昧性回避)。
  **テスト(実ソケット・実TLS)**: `real_quic_roundtrip_over_loopback`
  (127.0.0.1のループバックで実UDPソケット上に実QUIC接続を張り、TLS1.3
  ハンドシェイク+双方向ストリームでのJSON往復を実証)、
  `connect_to_unreachable_quic_destination_errors_without_hanging`
  (誰も応答しない宛先へ接続してもハングせずタイムアウト/エラーで返る
  ことを実証。`ClientConfig`に`max_idle_timeout(3s)`を設定して実現)。
  Multipath QUIC(MPQUIC)ではなく単一経路QUICの実装であることを
  モジュールdocに明記(拡張要件(3)④の役割分担どおり、物理経路の
  マルチホーミングはMPTCP/SCTP側の担当と整理)。
  **② PostgreSQL WAL (`open-web-server-ledger::PostgresWal`)**: `sqlx` 0.8
  (runtime-tokio-rustls + postgres + chrono features、`open-runo-db`と
  同じバージョン指定に揃えた)を新規追加。既存の`WriteAheadLog`トレイトへの
  実装として、`append`は`pool.begin()`→`INSERT ... ON CONFLICT
  (idempotency_key) DO NOTHING`→`tx.commit()`の実トランザクション、
  `mark_committed`も同様に実`BEGIN`/`COMMIT`で`UPDATE`する。
  `SCHEMA_SQL`定数(`CREATE TABLE IF NOT EXISTS ledger_mutations`)と
  `ensure_schema()`ヘルパーを同梱。
  **検証の限界(正直な記載)**: この開発環境(Windowsサンドボックス)には
  到達可能なPostgreSQLインスタンスが無く、`docker-compose.yml`もこの
  リポジトリには存在せず、`pg_isready`コマンドも未インストールであることを
  確認した。そのため**実DBに対する統合テストは実施できていない**——
  代わりに(a)SQL文字列そのもの(ON CONFLICT句・バインドパラメータ数・
  スキーマのPRIMARY KEY列とON CONFLICTターゲットの一致)を検証する単体
  テスト4件、(b)`DATABASE_URL`環境変数が設定されている場合にのみ実行
  される`#[ignore]`付き統合テスト1件(`cargo test -- --ignored`で
  到達可能なPostgreSQLがある環境なら実際にBEGIN/COMMITを検証できる)、
  の2段構えで検証可能性を確保した。これは正当なブロッカーであり、
  実行できたと偽って報告しない。
  **未着手として明記**: ④MPTCP/SCTP(拡張要件(3)の最後の伝送路)、
  ②aruaru-db・③マルチリージョン同期レプリケーション・④独立監査
  トランザクションログ(拡張要件(4)の残り3系統)、aruaru-db×ZFS
  スナップショット連携(拡張要件(2)の次回新規開発予定)は今回未着手。
  スコープが大きく(2リポジトリ以上の内部構造をまたぐ、または本格的な
  マルチホーミング実装が必要)、1パスで安全に検証可能な単位に収まらないと
  判断し次回以降へ回した——「未着手だから見送る」という判断のみで
  終わらせず、まず着手を試み(QUIC・PostgreSQLの2項目は実装完了)、
  真に大きすぎる残り項目は理由を明記して先送りした。
  **ビルド確認**: `cargo check --workspace` / `cargo test --workspace` は
  全24テスト(23 green + 1 ignored)を確認。ワークスペース依存に
  `quinn`/`rcgen`/`sqlx`を追加(`open-web-server-wire`/`open-web-server-ledger`
  のCargo.tomlへ配線)。
  **ドキュメント**: `README.md`(ルート)柱6の説明を更新。10ヶ国語+日本語の
  全11 README、`PORTING.md`(4.3/4.4節を新設)を更新。この`CLAUDE.md`の
  拡張要件(3)進捗ノート・「現状」節のテスト数を更新。
- **2026-07-11 (前回、open-web-server-wireを3層防御通信→4層防御通信へ拡張、
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

## HANDOFF追記(2026-07-15) — appserver連携(第二のApache→第二のTomcat配線)

- 姉妹リポジトリ(open-runo / poem-cosmo-tauri)に `open-runo-appserver` が
  新設された(§0.9)。本リポジトリの `TenantRegistry` と接続するための
  型非依存ブリッジ `tenant_bridge::dispatcher_from_tenants` が先方に用意済み:
  `registry.list()` の各 `TenantConfig` から `(host, backend_addr)` ペア列を
  作って渡すだけで、不変・ロック不要の `TenantDispatcher`(読み取りが
  マルチコアでスケール)が得られる。解析不能なbackend_addrは拒否リストで
  返るので、起動時に監査ログへ記録すること(黙殺禁止 — §0)。
- 配線方法はPCセッションで選択: Cargo git依存
  (`open-runo-appserver = { git = "https://github.com/aon-co-jp/open-runo" }`)
  またはワークスペース外パス依存。sandboxではクロスリポジトリビルド未検証。

## HANDOFF追記(2026-07-15) — poem/indexmap MSRV修正は対象外の確認

- 姉妹リポジトリ(poem-cosmo-tauri/open-runo)で発見・修正した
  「`rust-version=1.75`宣言と`poem`未ピンの不整合」バグは、本リポジトリ
  (`rust-version = "1.80"`、poem不使用)には該当しない。対応不要。

## HANDOFF追記(2026-07-19、audiocafe-tokyo-rustユーザーからの指示による次回着手事項)

- `audiocafe-tokyo-rust`のユーザーから、エコシステム内の各リポジトリ
  (RPoem・open-web-server・RTypeScript・RReact等)について「未着手や
  未完成の技術があれば、それぞれのリポジトリもTESTしながら完成させて
  いって、実用性と完成度を高めていって下さい」という要望があった。
  ユーザー確認の上、各リポジトリを**別セッションで順に実施**する方針と
  なったため、本セッションでは実装は行わず、次回このリポジトリの
  セッションが開始した際の着手事項としてここに記録するに留める。
  - **現状の認識(上記「アプリケーションサーバー層の役割」より)**:
    `open-web-server`は「Apache＋Nginxのハイブリッド仕様のWebサーバー」
    として構想されているが、**まだその役割を実際には果たせていない**と
    このファイル自身に明記されている——現時点で最も明確な「未完成」
    ポイント。
  - **次回セッションで確認すべきこと**: (1) 現状の実装が実際に
    Apache/Nginx相当のどの機能(vhost・リバースプロキシ・静的配信・
    SSL終端等)まで到達しているか、`cargo test`を実際に実行して
    現在地を棚卸しする。(2) 未実装の中核機能を洗い出し、優先度の
    高いものから実装→実際に起動して動作確認(型チェックのみで
    「完了」としない、既存の検証徹底ルールを踏襲)→テスト追加、の
    サイクルを回す。(3) `open-runo`/`poem-cosmo-tauri`が現状
    代替として担っている役割を、本来`open-web-server`がどこまで
    引き継げるかの現実的なロードマップを整理する。

## アプリケーションサーバー層の役割(open-runo / poem-cosmo-tauri、2026-07-16追記)

「配信エンジン(vhost)」に`open-web-server`を選択肢として追加したが、
open-web-serverがApache＋Nginxのハイブリッド仕様のWebサーバーとして
まだ機能していない間は、Tomcatのような互換レイヤーとして機能するのは
`open-runo`または`poem-cosmo-tauri`である。

これらは`open-raid-z`とVersionlessAPIによって、バージョンレス運用と
バージョン管理・Git管理を両立しながら、ACID互換性とZFS互換性に対応した
`aruaru-db`と、PostgreSQLとのDUAL DATABASE構成による「4層4重」の
最新鋭の通信システムを構築し、仕様変更が容易なデータベース設計により、
3DオンラインゲームAI課金アイテム、オンライン金融、オンライン証券、
オンラインクレジットカード決済など、ネット上で紛失してはならない
ミッションクリティカルな用途向けに、24時間365日ノンストップの
サーバー対応WEBサイト開発を全面的にバックアップするフレームワーク・
ミドルウェアとして機能することを目指す。
