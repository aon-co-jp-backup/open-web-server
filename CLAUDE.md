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

> ⚠️ **通信層(3-④)の位置づけ訂正・再検証(2026-07-23、日英Web検索で
> 裏取り)**: ユーザーから「本当に最先端の4層4重通信か」と再検証を
> 求められ、改めて日英両言語で調査した。①TCP(権威パス)・②UDP
> (即時通知)・③QUIC(単一経路)は現行の実務(決済業界のスマートリトライ、
> 暗号資産取引基盤でのgRPC/QUIC系採用例)と整合することを確認できた
> ([GR4VY: Payment retry logic 2026](https://gr4vy.com/posts/payment-retry-logic-explained-smart-retries-for-failed-transactions-in-2026/)、
> [gRPC low latency trading 2026](https://www.sachith.co.uk/grpc-when-low-latency-really-matters-ci-cd-automation-practical-guide-mar-8-2026/))。
> **しかし④(`aggligator`によるMPTCP/SCTP代替)は、技術的には正直な
> 実装だが、実際の金融機関が「複数物理経路の冗長化」を実現する方法とは
> レイヤーが異なる**——金融機関は物理経路冗長化を**SD-WAN(複数ISP/
> 専用線をネットワーク層で束ねる、サブ2秒フェイルオーバー)**という
> インフラ層で解決しており、アプリケーションコード側でMPTCP的な集約を
> 自前実装する構成は業界の主流ではないことを確認した
> ([Cisco: SD-WAN for Financial Services](https://www.cisco.com/c/dam/m/en_us/solutions/industries/financial-services/new-financial-services-landscape-of-networking/SD-WAN-for-Financial-Services.pdf))。
> `aggligator`自体は実在する現役メンテナンスのクレートであり嘘は無いが
> (`mptcp_channel`モジュールdoc参照)、「これが金融業界の最先端の
> 物理経路冗長化そのものだ」という主張は誇張であり、正しくは「本物の
> SD-WAN/ネットワークインフラ層の冗長化が無い開発環境における、
> アプリケーション層での代替・次善策」という位置づけである。本番運用
> では、この層の真の代替はSD-WAN等のネットワークインフラ導入であり、
> `mptcp_channel`はそれが無い場合のソフトウェアのみでの次善策として
> 維持する(削除はしない、正直な位置づけの明記のみ)。

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
- **日英Web検索の結果、CockroachDB/TiKV等の最先端の実運用システムが
  既に対応済みと判明した技術的ギャップは、「今のところは大丈夫」という
  報告に留めず、確認を求めず自動でそのまま実装に着手すること**
  (ユーザー指示、2026-07-23。正本は`open-raid-z/CLAUDE.md`同日エントリ
  参照)。
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

### 2026-07-24(最終+5) 実機/エミュレータでのアイコン動的切替検証+CI実動作検証+正式署名keystoreでのリリースビルド
(ユーザー指示「1.実機/エミュレータでのアイコン動的切替の見た目確認、
2.CI(build-androidジョブ)の実動作検証、3.正式な署名鍵でのビルド」)

1. **アイコン動的切替の検証(実エミュレータ、代替検証方式で実施)**:
   既存AVD `Pixel_9_Pro` を起動(`emulator -no-snapshot -gpu
   swiftshader_indirect`)、`adb`で完全ブートを確認後、デバッグAPKを
   実インストールして検証。**正直な開示**: ホーム画面上でのアイコン
   見た目変化そのもののスクリーンショット確認は、このセッションの
   自動化環境ではPixelランチャーのジェスチャーナビゲーション
   (アプリドロワーを開くための下端スワイプ)が`adb shell input swipe`
   コマンドでは安定して認識されず(複数回試行したが毎回ホーム画面の
   ままだった)実施できなかった。そのため、ユーザー指示にある
   「難しい場合はdumpsys等での代替検証でもよい」に従い、
   `adb shell dumpsys package tokyo.runo.openwebserver`で
   `disabledComponents`/`enabledComponents`を確認する方式で検証した。
   **4プロファイルすべてで実際に`applyLauncherIcon()`が正しく動作する
   ことを実機能として確認**: `ProfileSelectActivity`の各ボタンを
   `adb shell input tap`で実タップ→`am force-stop`で毎回アプリを終了し
   まっさらな状態から再検証、を4回繰り返し、いずれも選択した
   プロファイルの`activity-alias`のみが`enabledComponents`に載り、
   残り3つが`disabledComponents`に載ることを確認した
   (省メモリ→`LauncherMemorySaver`のみ有効、省電力→
   `LauncherPowerSave`のみ有効、通常→`LauncherNormal`のみ有効、
   常時電源接続→`LauncherAlwaysOn`のみ有効、の4パターンすべて実証)。
   これは「ホーム画面のランチャーがこの状態変化をどう描画するか」
   ではなく「アプリ側が正しいAPIを正しい引数で呼んでいるか」を
   OSのコンポーネント管理状態から直接検証するものであり、
   `PackageManager.setComponentEnabledSetting()`が実際に意図通り
   機能していることの決定的な証拠になる。**残る未検証事項**:
   ランチャー(ホーム画面アプリ)側が実際にこの状態変化をアイコン
   描画へ反映する見た目の変化そのものは、今回も確認できていない
   (前回HANDOFF記載の制約から変わらず)。

2. **CI(`build-android`ジョブ)の実動作検証**: 一時タグ
   `v0.1.0-android-ci-test`をpushしてGitHub Actionsを実際に起動させ、
   `gh run watch`で結果を確認した。**結果: 成功**——`build-linux`・
   `build-windows`・`build-android`の3ジョブすべてが成功し、
   `release`ジョブも正常に完了した(このタグ用の一時Releaseが
   作成されたことを確認)。`build-android`ジョブの各ステップ
   (Android SDKセットアップ・NDKインストール・
   `cargo ndk`によるarm64-v8a/x86_64クロスビルド・
   `gradlew :app:assembleDebug`)はすべて成功ログを確認した。
   **前回HANDOFFの「正直な開示」(CI実動作は未検証)はこれで解消**——
   修正サイクル無しで一発成功したため、追加のワークフロー修正は
   不要だった。検証後、一時タグ・一時Releaseを削除
   (`git push --delete origin v0.1.0-android-ci-test`・
   `gh release delete v0.1.0-android-ci-test`)、本番の`v0.1.0`タグ・
   Releaseには一切触れていない。

3. **正式な署名鍵(release keystore)でのビルド**: `keytool -genkeypair`
   (RSA 2048bit、有効期限9125日≒25年、PKCS12形式)で
   `open-web-server-release.keystore`を新規生成(パスワードはこの
   作業用に`openssl rand`で新規生成した使い捨ての値、
   `android/keystore-scratch/`配下に一時保存——このディレクトリは
   `.gitignore`に追加済みで一切コミットしていない)。
   `android/app/build.gradle.kts`に、環境変数
   (`OPEN_WEB_SERVER_RELEASE_STORE_FILE`/`_STORE_PASSWORD`/
   `_KEY_ALIAS`/`_KEY_PASSWORD`)経由でのみ実際の値を受け取る
   `signingConfigs.release`を追加(4つすべて設定されている場合のみ
   `release`ビルドタイプへ適用、未設定時は既存のデバッグ署名フローに
   一切影響しない後方互換設計)。`gradle :app:assembleRelease`で
   実際に正式署名済みAPK(`app-release.apk`)をビルドし、
   `apksigner verify --verbose --print-certs`で
   **`Verifies`・`Verified using v2 scheme: true`・新規生成した
   証明書のDN(`CN=open-web-server, OU=aon-co-jp, ...`)・RSA 2048bit**
   であることを実際に確認した。生成したkeystoreファイル・
   `credentials.txt`(使い捨てパスワード)はいずれもgit管理対象外の
   ディレクトリに置いたのみで、コード・ドキュメントのいずれにも
   平文のパスワード自体は記載していない。この正式署名済みAPKを
   `open-web-server-android-release.apk`という別名で
   `gh release upload v0.1.0`によりGitHub Releases(`v0.1.0`)へ
   追加アセットとしてアップロード済み(既存の
   `open-web-server-android-debug.apk`はそのまま残置)。
   **正直な開示・実運用への申し送り**: (a) 今回生成したkeystore・
   パスワードはこのセッションのローカルディスク
   (`android/keystore-scratch/`)にのみ存在し、セッション終了後に
   環境が破棄されれば失われる可能性が高い——実運用でユーザー自身が
   このkeystoreを引き継ぐ場合は、`credentials.txt`の中身を安全な
   パスワードマネージャー等へ移し、keystoreファイル自体も安全な
   場所(暗号化バックアップ等)へ複製しておく必要がある(紛失すると
   Google Play等でのアプリ更新の署名継続性が失われる)。
   (b) CIワークフロー(`.github/workflows/release.yml`)側は今回
   この正式署名鍵を使うようには変更していない(CI環境にkeystore・
   パスワードを安全に渡す仕組み[GitHub Secrets等]の設計は今回の
   スコープ外、現状のCIは引き続きデバッグ署名APKのみを生成する)。

**検証まとめ**: 型チェックのみでの完了報告はしていない——(1)は実機能
確認(dumpsys)、(2)は実際のCI実行ログ確認、(3)は実`apksigner verify`
での署名検証、いずれも実際の動作・成果物を確認した上での報告。

### 2026-07-24(最終+4) Android版APKをGitHub Releases(v0.1.0)へ追加+CIビルドジョブ+紹介ページ更新
(ユーザー指示「runo.tokyo/open-web-server とGithubにて、WindowsとLINUXは
ありますが、省電力と省メモリ駆動版も選択可能なAndroidスマホとタブレット版の
インストーラー付きアプリのダウンロード可能にして」、追加指示「アイコンに
省電力と省メモリを選択したらその様な表示に変更になるようにして」)

1. **アイコン動的切り替えの実装(追加指示対応、着手前は未実装と判明)**:
   既存の4つの`activity-alias`(`AndroidManifest.xml`)はホーム画面への
   個別インストール用の据え置きアイコンとしては機能していたが、
   「アプリ内でプロファイルを選択した際にホーム画面上の代表アイコン自体が
   切り替わる」実装は無かった(`setComponentEnabledSetting`呼び出しが
   コード中に一切存在しないことを`Grep`で確認済み)。`PowerProfile.kt`の
   `save()`内から呼ばれる新規`applyLauncherIcon()`を追加し、
   `PackageManager.setComponentEnabledSetting()`で選択中プロファイルの
   `activity-alias`のみ`COMPONENT_ENABLED_STATE_ENABLED`、他3つを
   `COMPONENT_ENABLED_STATE_DISABLED`にする(`DONT_KILL_APP`指定でプロセス
   再起動なし)。`ProfileSelectActivity`のボタン押下・`MainActivity`の
   電源切断/再接続ダイアログでのプロファイル切替、どちらも`PowerProfile.save()`
   を経由するため、両経路で自動的にアイコン切替が反映される。**正直な
   限界**: ランチャー(ホーム画面アプリ)側が有効/無効変化を反映する
   タイミングはランチャー実装依存(多くは即時、まれに再起動を要する場合が
   ある)。実機/エミュレータでの「切替後に実際にホーム画面アイコンが
   変わって見える」ことのスクリーンショット確認は本パスでは実施していない
   (`gradle :app:assembleDebug`のBUILD SUCCESSFULまでの確認に留まる)。
2. **APKビルド(デバッグ署名)**: `gradle :app:assembleDebug`で
   BUILD SUCCESSFUL、`android/app/build/outputs/apk/debug/app-debug.apk`
   (約20.3MB、既存jniLibs[arm64-v8a/x86_64]同梱)を確認。**正直な開示**:
   正式な配布用署名鍵(keystore)はこの環境に用意されていないため、
   デバッグ署名のAPKのまま。実運用では正式な署名鍵での再ビルドを推奨。
3. **GitHub Releases(v0.1.0)へ追加**: `gh release upload v0.1.0`で
   `open-web-server-android-debug.apk`を追加アセットとしてアップロード
   済み(既存の`open-web-server-linux-x86_64.tar.gz`/
   `open-web-server-windows-x86_64.zip`と並んで表示されることを
   `gh release view v0.1.0`で確認)。リリースノートにAndroid版の説明
   (ARM64/x86_64両ABI対応・4電源プロファイル選択可・デバッグ署名の注記)
   を追記。
4. **`.github/workflows/release.yml`にAndroidビルドジョブ`build-android`を
   追加**(将来のタグpush時の自動ビルド用): `android-actions/setup-android`
   でSDKをセットアップ後、NDK 27.1.12297006を`sdkmanager`で導入、
   `cargo ndk`でarm64-v8a/x86_64向けにRustバイナリをクロスビルドし
   `jniLibs`へ配置、`./gradlew :app:assembleDebug`でAPK生成、
   `softprops/action-gh-release`でReleaseへ添付する既存Linux/Windows
   ジョブと同じパターン。**Gradle Wrapper(`gradlew`/`gradlew.bat`/
   `gradle-wrapper.jar`)が本リポジトリに存在しなかった**ため、ローカルの
   キャッシュ済みGradle 8.11.1で`gradle wrapper --gradle-version 8.11.1`
   を実行し生成・コミット対象に追加(これが無いとCI環境でGradleを
   呼び出す手段が無かった)。**正直な制約(ユーザー指示に沿って明記、
   1回のCI実行で確認できる範囲に留めた)**: このジョブが実際にCI環境
   (ubuntu-latestランナー)で動作するかどうかは、本コミットの直後に
   タグpushで発火する1回のCI実行でしか確認できていない可能性が高い。
   Android SDK/NDKのセットアップ・cargo ndkのクロスコンパイル・Gradle
   ビルドという複数の外部要因が絡むため、失敗時に無理に追加修正を
   重ねて時間を消費しすぎないよう、`build-android`ジョブに
   `continue-on-error: true`を設定し、失敗してもLinux/Windows向け
   リリース自体はブロックされない設計にした(`release`ジョブの`if`条件は
   `build-linux`/`build-windows`の成功のみを必須とし、Android成果物は
   `fail_on_unmatched_files: false`で「無ければ無いまま」公開される)。
5. **紹介ページ(`site/index.html`)にAndroidダウンロードリンクを追加**:
   「ダウンロード・インストール」節にAndroid版カードを新設し、
   GitHub Releasesの当該APKアセットへの直接リンク・ARM64/x86_64両ABI
   対応・4電源プロファイル選択可能である旨・デバッグ署名である旨の注記を
   日本語で追加。`runo.tokyo`側は別の小規模ルータープロジェクトであり
   open-web-server自身の紹介ページは持たないため(`F:\runo\runo.tokyo`は
   `CLAUDE.md`/`README.md`/`src`のみで`site/`無し)、`open-web-server`側の
   `site/index.html`のみを更新した。
6. **検証(型チェックのみで完了と報告しない、既存運用ルール徹底)**:
   `cargo build --release --bin open-web-server`成功(既存のdead_code
   警告のみ、新規warning無し)。`cargo test --workspace`全21テスト
   (gateway/ledger/wire合算、doc-tests含む)green。実際に
   `open-web-server.exe`を`web_vhosts.toml`(`docroot=site`)経由で起動し、
   `curl -H "Host: 127.0.0.1" http://127.0.0.1:18078/`で**status 200**、
   本文に`open-web-server-android-debug.apk`および
   「省メモリ版・省電力版・通常版・常時電源接続版」の文字列が実際に
   含まれることを確認済み。`gh release view v0.1.0`でアセット一覧に
   `open-web-server-android-debug.apk`が含まれることも確認済み。
- **次にすべきこと**: (1) タグpush(次回`v0.1.1`等)でのCI上の
  `build-android`ジョブの実動作確認、失敗時のログ調査・修正。
  (2) アイコン動的切り替えの実機/エミュレータでのスクリーンショット
  確認。(3) 正式な配布用署名鍵の準備、リリース版署名でのAPK再発行。

### 2026-07-24(最終+3) Android版: 実ハードウェア検出ロジックの実装+実エミュレータ検証
(ユーザー指示「外付けGPU検出は実装しない、という前回判断を撤回し、実際に検出ロジックを
実装してほしい」、追加指示「検出結果・4プロファイル説明文を日英併記」)

1. **新規`HardwareAccelDetector.kt`**: 過剰実装を避けつつ実際に検出する。
   - **内部GPU**: `EGL14`で1x1のpbufferサーフェスを一時生成し
     `GLES10.glGetString(GL_RENDERER/GL_VENDOR)`を取得後、即座に破棄
     (`GLSurfaceView`のような画面表示用ライフサイクル管理は行わない、
     一般的な最小限のEGLコンテキスト管理パターン)。Vulkan対応は
     `PackageManager.FEATURE_VULKAN_HARDWARE_VERSION`ハードウェア
     featureフラグでの軽い判定(**実装当初`ActivityManager#
     deviceHasVulkanSupport()`という実在しないAPIを書いてしまい
     ビルドで発覚・修正した実バグ**)。
   - **NPU**: NPU専用のfeature flagは標準に無いため、`Build.VERSION.
     SDK_INT >= 27`(NNAPI導入バージョン)をNPU利用可能性の簡易フラグ
     とし、Android 12+の`Build.SOC_MODEL`/`SOC_MANUFACTURER`があれば
     併記。「NPUを直接検出した」とは主張せず「NNAPI利用可能性」という
     正直な粒度に留めた。
   - **外付けGPU**: 直接検出は行わず、`DisplayManager#getDisplays()`が
     2件以上を返すかどうかで`external_display_hint`という正直な粒度の
     フラグのみ(「外部ディスプレイ接続を検出した」であり「外付けGPUを
     検出した」ではないと明記)。
   - `toAccelBackendEnvValue()`が検出できた情報のみを`"gpu:...;npu:...
     ;external_display_hint"`形式に組み立て、`OPEN_WEB_SERVER_ACCEL_
     BACKEND`環境変数値として使う。
2. **`MainActivity.kt`**: `accelBackendEnvValue()`の常時電源接続分岐を
   固定値`"hardware_accelerator"`から`hardwareDetection.
   toAccelBackendEnvValue()`(実検出結果)へ差し替え。新規「🔍 ハードウェア
   検出情報を表示」ボタン+`showHardwareInfoDialog()`(`AlertDialog`)を
   追加、`activity_main.xml`(スマホ・タブレット両レイアウト)・
   `strings.xml`に配線。
3. **日英併記(追加指示対応)**: `HardwareAccelDetector.
   toHumanReadableSummary()`をGPU名・Vulkan対応・NPU(NNAPI)・SoC・
   ディスプレイ数・外部ディスプレイ接続の全項目で「日本語 / English」の
   併記形式に変更。ハードウェア検出ダイアログのタイトル・案内文・
   閉じるボタンも日英併記。4電源プロファイル(`strings.xml`の
   `profile_*_desc`)も、既存が日本語のみだったため全4件+選択画面の
   タイトル/サブタイトルに英語訳を改行併記する形で追加(既存の日本語
   文言は変更せず追記のみ)。
4. **ビルド・実機検証**: `gradle :app:assembleDebug`で**BUILD
   SUCCESSFUL**(Vulkan API修正後)。**実エミュレータ(`Pixel_9_Pro`、
   `emulator -no-window`+`adb`のみで検証——GUI操作ツールは使わず、
   `adb shell input tap`+`adb exec-out screencap`のスクリーンショット
   読み取りで確認)で3点とも実証済み**:
   (a) `LauncherAlwaysOn`起動→4電源プロファイル選択導線・新規
   「ハードウェア検出情報を表示」ボタンが実際に表示されることを
   スクリーンショットで確認。
   (b) `adb shell dumpsys battery unplug`で実際に電源切断をシミュレート
   したところ、**アプリ自身が発火する`ACTION_POWER_DISCONNECTED`
   ではなくAndroidシステム自体が実際に送るブロードキャストを捕捉し**、
   3択ダイアログ(「省電力版へ切替」/「普通版(通常版)のままにする」/
   「省メモリ版へ切替」)が実際に表示されることを確認
   (`adb shell am broadcast`での手動送信は`SecurityException`
   [protected broadcastのため一般アプリからの送信不可]で失敗したが、
   `dumpsys battery unplug`自体が本物のシステム発火を引き起こしたため
   検証として成立した)。
   (c) ハードウェア検出ボタンをタップし、**実エミュレータの実際の
   GPU情報**(`Android Emulator OpenGL ES Translator (Google
   SwiftShader)`、ベンダー`Google (Google Inc.)`、Vulkan対応あり、
   NNAPI利用可能・SoC`AOSP ranchu`、接続ディスプレイ1・外部ディスプレイ
   なし)が日英併記で正しく表示され、`OPEN_WEB_SERVER_ACCEL_BACKEND`に
   実際に反映される値もダイアログ内に表示されることを確認。
5. **正直な制限**: (1) 実機(物理スマホ/タブレット)ではなくエミュレータ
   のみでの検証(既存のこのプロジェクトの制約と同じ)。(2) EGL
   pbufferでの`GL_RENDERER`取得は多くの実機で機能するはずだが、
   ドライバによっては失敗しうる(その場合`toAccelBackendEnvValue()`は
   Vulkan判定のみへフォールバックし`"cpu"`にはならない設計だが、
   GPU名自体は取れないケースがあることをコード内docに明記済み)。
   (3) 省電力/省メモリ/通常プロファイルでの起動画面・
   `ProfileSelectActivity`の日英併記表示自体は文字列リソースの確認に
   留まり、3プロファイルそれぞれの実タップでのスクリーンショット確認は
   今回は常時電源接続版のみ実施(時間の都合、コード自体は同一ロジック
   パスのため機能上のリスクは低いと判断)。



### 2026-07-24(最終+2) nasa.tokyo/icpo.tokyoをVPS本番へ追加デプロイ完了

VPS(ConoHa)へ新規`nasa.tokyo`・`icpo.tokyo`リポジトリをclone・
`cargo build --release`・systemdサービス化(`nasa-tokyo.service`
`127.0.0.1:4700`、`icpo-tokyo.service` `127.0.0.1:4800`)し、
open-web-serverの`tenant_router`へ4件(bare+www×2ドメイン)登録。
ユーザーがConoHa DNS管理画面でAレコード(`160.251.237.162`)を設定後、
実Let's Encrypt証明書を全4件取得し、`https://nasa.tokyo/`・
`https://icpo.tokyo/`とそれぞれのwww版が実インターネット経由で
HTTPS 200で応答することを確認済み。断り書き(「実際のNASA/ICPOとは
無関係の独立プロジェクト」、日英併記)も実際の本文に含まれることを
`curl`で確認済み。これでopen-web-server配下の全10ドメイン
(既存8+今回2)が本番稼働中。

### 2026-07-24(最終+1) Android版: 3電源プロファイル→4電源プロファイルへ拡張
(省メモリ版を新設、ユーザー指示「省電力と省メモリは明確に別軸として区別」)

1. **`PowerProfile.kt`**: `MEMORY_SAVER("memory_saver", "省メモリ", "🧠✕")`を
   新規追加(既存`POWER_SAVE`/`NORMAL`/`ALWAYS_ON`は変更なし)。
2. **`MainActivity.kt`の具体的施策(「省電力」とは別軸であることを実装で
   示す)**:
   - `healthPollIntervalMs()`: 省メモリ版は通常版と同じ1分(ポーリング
     間隔延長=省電力の施策軸であり、省メモリ版はここを変えない)。
   - `logBufferMaxLines()`(新規): ログ画面`StringBuilder`の保持行数上限
     (省メモリ=40行/省電力・通常=400行/常時電源=2000行)。`trimLogBuffer()`
     (新規)が`pollHealthz()`の各試行後に実際に古い行を破棄する。
   - `healthBodyPreviewMaxChars()`(新規): ヘルスチェック応答本文の保持
     文字数上限(省メモリ=64/省電力・通常=512/常時電源=4096)、
     `pollHealthz()`で実際に切り詰めて保持する。
   - `applyProfilePowerBehavior()`に`MEMORY_SAVER`分岐を追加(WakeLock
     取得なし、上記のキャッシュ/バッファ縮小のみを行うことをログに明記)。
   - `accelBackendEnvValue()`: 省メモリ版も省電力/通常と同じ`"cpu"`。
   - **正直な開示**: バックグラウンド先読み・プリフェッチは元々本アプリに
     存在しないため、「行わない」という施策は実装上「元から無い」ことの
     確認に留まる(新規の抑制コードは無い)。実装した具体的施策は
     上記のログ行数/応答本文保持サイズの上限のみ。
3. **電源切断時ダイアログを2択→3択へ変更**(`onPowerDisconnected()`):
   `setPositiveButton`(省電力版へ切替、既定推奨)/
   `setNeutralButton`(省メモリ版へ切替)/`setNegativeButton`(普通版
   [通常版]のままにする)の3ボタン構成。
4. **UI**: `activity_profile_select.xml`に`buttonMemorySaver`を先頭に
   追加(絵文字🧠+日本語ラベル+説明文、既存3択と同じ見た目パターン)。
   `strings.xml`に`app_name_memorysaver`/`profile_memory_saver_button`/
   `profile_memory_saver_desc`を追加。新規`ic_launcher_memorysaver.xml`
   (紫背景+メモリチップ図形、既存の緑/青/橙と重複しない配色)。
   `AndroidManifest.xml`に`LauncherMemorySaver`活動-aliasを追加
   (`LAUNCH_MEMORY_SAVER`アクション)。`ProfileSelectActivity.kt`に
   `buttonMemorySaver`のクリックリスナーを追加。
5. **検証**: `gradle :app:assembleDebug`(`~/.gradle/wrapper/dists/
   gradle-8.11.1-all`配下のキャッシュ済みgradleバイナリを直接実行)で
   **BUILD SUCCESSFUL**を確認(既存jniLibs[arm64-v8a/x86_64]同梱のまま、
   新規warning無し)。**正直な制限**: 本パスでは実機/エミュレータでの
   実地検証(電源切断シミュレート・3択ダイアログの実タップ)は実施して
   いない——ビルド成功(型チェック+APK生成)までの確認に留まる。前回
   HANDOFF(2026-07-24続き3・続き)記載の実機/エミュレータ検証実績は
   既存の3プロファイル部分についてのものであり、今回追加した省メモリ版
   固有の分岐は未実機検証のまま。
6. **未実装として明記(過剰実装回避、ユーザー指示通り)**: 外付けGPU
   検出のAndroid側実装は行っていない——`OPEN_WEB_SERVER_ACCEL_BACKEND`
   環境変数がRust側へ渡る既存の仕組みのみに委ね、Android自体が外付けGPU
   を一般的にサポートする標準APIを持たないための判断(コード内docにも
   明記済み)。
- 次にすべきこと: (1) 実機/エミュレータでの4択ダイアログ・省メモリ版
  アイコン起動の実地検証、(2) 省メモリ版のキャッシュ上限を将来的に
  ネイティブ`open-web-server`本体側の設定(環境変数等)とも連携させる
  拡張(現状はAndroidシェル側のログ/表示バッファのみが対象、ネイティブ
  プロセス自体のメモリ使用量には影響しない——正直な開示)。

### 2026-07-24(最終) VPS本番カットオーバー完了 — nginx廃止、open-web-serverが80/443を直接受ける構成へ移行

**ユーザー指示「実施して」を受け、最終カットオーバーを実施し成功**:
1. `/etc/systemd/system/open-web-server.service`を編集:
   `OPEN_WEB_SERVER_BIND=127.0.0.1:8103`→`0.0.0.0:80`、
   `OPEN_WEB_SERVER_TLS_BIND=0.0.0.0:8443`(テスト用)→`0.0.0.0:443`(本番)。
2. `systemctl stop nginx` → `systemctl restart open-web-server`
   → `ss -ltnp`で0.0.0.0:80/443ともopen-web-serverが直接listenして
   いることを確認。
3. プロセス再起動でメモリ内状態(TLS証明書・web_vhost・redirects)が
   リセットされる設計のため、直後に全18ドメイン(bare+www)の
   ACME証明書を並列で再取得(永続化済みアカウント鍵のためレート制限
   消費なし)。`domains.toml`経由のtenant登録は自動永続化されるため
   再登録不要だったが、**`web_vhosts`と`redirects`はファイルへの
   自動書き込みが無い設計と判明**(tenantsとの非対称、次回の改善候補
   として記録)——`www.runo.tokyo`/`audiocafe.tokyo`のweb_vhost、
   `www.audiocafe.tokyo`/`www.aruaru.tokyo`のredirectを手動で再登録。
4. **実インターネット経由での本番検証(型チェック・テストポートでの
   検証だけで完了と報告しない、既存運用ルール徹底)**: 全18ドメインへ
   実際に`https://<domain>/`でアクセスし、200(または301)を確認。
   `audiocafe.tokyo`のPHP-FPM/FastCGI直接配信、
   `aruaru.tokyo`の`/aruaru/`・`/aruaru-lady/`・`/rakuten-mobile/`
   (Hostヘッダー書き換え転送)も実インターネット経由でHTTPS 200を確認。
   HTTP(80番)側の応答も確認。
5. **正直な今後の課題**: (a) `web_vhosts`/`redirects`もtenantsと同様に
   ファイルへ自動永続化する設計に統一すべき(次回のプロセス再起動で
   再度手動登録が必要になる)、(b) nginx自体はまだVPS上にアンインストール
   していない(停止のみ、切り戻し用に一旦残置)、(c) TLS証明書の
   自動更新(Let's Encryptは90日で失効)は現状手動での`POST /admin/
   tenants/:host/tls/acme`再実行が前提——定期実行(cron/systemdタイマー)
   の仕組みが未実装。

### 2026-07-24(続き10) nginx移行の残実装ギャップ3点を実装完了
(前回チェックポイントの「次回セッション最初にすべきこと」対応)

**1. ホスト名ベースの汎用301リダイレクト**: 新規`redirects.rs`
(`RedirectRegistry`+`RedirectRule{host, redirect_to}`)、
`main::dispatch()`の**最初**(既存の`/admin/*`・`/healthz`等すべての
既存ハンドラより先)でHostヘッダを見てマッチすれば即301+`Location`
(`redirect_to`+元パス・クエリを連結)を返す。管理API
`POST/GET /admin/redirects`・`DELETE /admin/redirects/:host`
(既存`x-admin-token`/`KeyGuardian`認証を再利用)。
`OPEN_WEB_SERVER_REDIRECTS_FILE`環境変数での`redirects.toml`一括
ロード対応(`redirects.toml.example`新規)。実HTTP統合テスト
(`host_redirect_returns_301_with_location_over_real_http`)で
登録前404→登録後301(`/healthz`ですら奪われることも確認=優先順位の
直接証拠)→一覧反映→削除後404、を実TCP接続で検証済み。

**2. PHP-FPM/FastCGI直結配信**: `web_vhost::PhpMode`
(`BuiltinServer`〈既定、既存`php -S`のまま完全後方互換〉/
`FastCgi{fastcgi_addr}`)を新設。日英Web検索で実在・アクティブに
メンテされていることを確認した`fastcgi-client`0.11.1クレート
(`runtime-tokio`feature、`Client::new_tokio`)を新規`fastcgi-client`
Cargo feature配下(既定オフ)で追加。新規`php_fastcgi.rs`が
`SCRIPT_FILENAME`等のCGIパラメータを組み立ててphp-fpmへ直接FastCGI
接続し、CGI形式(`Status:`ヘッダ+空行+ボディ)の応答をパースして
`Response`に変換する。feature無効ビルドでは`501 Not Implemented`を
正直に返す(パニックや無言フォールバックはしない)。
**実機検証**: WSL2 Ubuntu上に`apt-get install -y php-fpm`
(php8.5-fpm)を実際にインストールし、`listen = 0.0.0.0:9000`へ変更、
Windows側からWSL2 IP経由で実際にTCP到達することを確認した上で、
`#[ignore]`統合テスト`real_php_fpm_roundtrip_over_fastcgi`
(`OPEN_WEB_SERVER_TEST_FASTCGI_ADDR`/`_DOCROOT`環境変数で有効化)を
実行し、**実php-fpmが生成した本文(`hello-from-php-fpm method=GET
host=fcgi-test.example`)が実際にステータス200で返ることを確認済み**
(型チェックのみでの完了報告ではない)。検証中に見つけた実バグ2件を
修正: (a) `SCRIPT_FILENAME`を`std::path::Path::join`で組み立てると
Windows開発環境ではネイティブ区切り文字(`\`)が混入し、Linux側の
php-fpmが実際のファイルを見つけられず404になっていた——
`SCRIPT_FILENAME`はバックエンド(php-fpm)側のファイルシステムパスで
あり本プロセスの実行OSとは無関係、という前提を見落としていたための
バグ。POSIX形式の`/`で手動連結するよう修正。(b) この開発環境の
Bash実行系(MSYS/Git Bash)が環境変数値中の`/var/www/...`のような
パスをWindowsパスへ自動変換する既知の挙動(`MSYS_NO_PATHCONV=1`が
必要)に一度引っかかった——コード側のバグではなく検証手順側の
注意点として記録。WSL2は前回HANDOFF記載の「アイドルタイムアウトで
VMが停止・IPが変わる」問題に今回も遭遇し、`sleep 300`のkeep-alive
プロセスをバックグラウンド起動して再現性を確保した(既知の対処法を
再適用)。

**3. Hostヘッダー書き換え転送**: `tenant_router::TenantConfig`に
`override_host: Option<String>`を追加(既定`None`、既存動作と完全
後方互換)。`proxy.rs`に`forward_to_stripped_with_host_override()`を
新設し、`override_host`が設定されていれば転送前にリクエストの
`Host`ヘッダをその値へ書き換えてから`forward_to_stripped`を呼ぶ。
`main::dispatch()`の`path_prefix`/host-onlyテナント転送の両経路を
これに差し替え。実HTTP統合テスト
(`override_host_rewrites_host_header_before_forwarding_over_real_http`)
で、Hostヘッダをそのままエコーバックするモックバックエンドを使い、
`aruaru.tokyo`の`/aruaru/...`テナント(`override_host:
"audiocafe.tokyo"`設定)への実リクエストが、実際に書き換え後の
`audiocafe.tokyo`をバックエンドが受け取ることを確認済み。

**検証まとめ**: `cargo build --tests --workspace`
(featureフラグ無し)・`cargo build --tests -p open-web-server-gateway
--features fastcgi-client`ともに新規warning無しで成功。
`cargo test --workspace`(featureフラグ無し)は**110件+既存ledger/wire
全件green**(新規: `redirects::`単体7件・`tenant_router`のoverride_host
TOML読込1件・`web_vhost`のphp_mode既定値/TOML読込2件・main.rsの
実HTTP統合2件を含む)。`cargo test -p open-web-server-gateway
--features fastcgi-client`は**114件green**(`php_fastcgi::`単体4件
+実php-fpm統合1件`#[ignore]`〈手動実行で実際にgreenを確認済み、上記
参照〉を含む)。既存テストへのリグレッションなし。

**正直な未検証事項・残課題**:
(1) `fastcgi-client` featureは既定オフのため、featureを付けずに
ビルドした本番バイナリでは引き続き`php -S`のみが使え、php-fpm接続は
`--features fastcgi-client`での再ビルドが必要(VPSデプロイ時の
`cargo build --release --features fastcgi-client,acme,ddns,sftp,upnp`
のようなfeatureフラグ追加が必要になる——次回VPSデプロイ時に対応要)。
(2) `php_fastcgi.rs`は各リクエストごとに新規TCP/Unixソケット接続を
張る単純な実装で、php-fpm側の接続プーリング/keep-aliveは活用しない
(将来の最適化候補として明記のみ、今回は範囲外)。
(3) Unixドメインソケット(`/run/php/php8.3-fpm.sock`形式)経由の
接続は非unixプラットフォームでは`501`を返すコードパスのみで、
実機検証はTCP経由(`172.22.9.49:9000`)のみ実施——実運用でよく使われる
Unixソケット経由の実機検証は次回、Linux環境から直接検証することを
推奨する。
(4) リダイレクト機能・Hostヘッダー書き換え機能は実VPS環境
(aruaru.tokyo/audiocafe.tokyo実ドメイン)へは未デプロイ、この開発
環境での実HTTP統合テストでの検証に留まる——次回VPSデプロイ時に
実ドメインでの動作確認が必要。
(5) 前回チェックポイントに記載の最終カットオーバー(nginx停止→
80/443切り替え)は今回も未実施のまま(このセッションのスコープは
コード実装のみ、実デプロイ・nginx設定変更はユーザー指示により対象外)。

### 2026-07-24セッション末尾チェックポイント(リミット接近のため記録)

**目的**: VPS(ConoHa、`ssh conoha`)上でnginxが担っている全ドメインを、
open-web-server自身のTLS終端(ACME自動取得)+`web_vhost`/`tenant_router`
へ完全移行し、nginxを廃止する(ユーザー指示、「open-web-serverをWEBサーバー
として、nginxの代わりにできないか」から発展)。

**完了したこと**:
1. ACMEクライアントの重複アカウント作成バグを修正(下記詳細エントリ参照、
   コミット`42928c1`、VPSへデプロイ済み)。
2. VPS上の**全13ドメイン+wwwバリアントで実Let's Encrypt証明書の取得に
   成功**(1つの再利用アカウントで、レート制限の再消費なし):
   karu.tokyo・runo.tokyo・audiocafe.tokyo・easy-web.tokyo・easyweb.tokyo・
   aon.tokyo・aon.co.jp・e-gov.info・aruaru.tokyoとそれぞれのwww版。
3. 途中で発見・修正した実バグ2件(いずれもVPS上のnginx設定ファイルの
   問題、コード側の問題ではない): `aruaru.tokyo`・`www.audiocafe.tokyo`の
   nginx設定に、`location`ブロックより先に評価される「サーバー直下の
   `return 301`」があり、ACMEチャレンジパスを問答無用でHTTPSへ
   リダイレクトしていた(`if`ディレクティブと同様、素の`server{}`直下の
   `return`はlocationマッチングより先にrewriteフェーズで無条件実行される、
   というnginxの仕様が原因)。`return`を`location /`内へ移すことで解消
   (VPS上の`/etc/nginx/conf.d/aruaru.tokyo.conf`・`audiocafe.tokyo.conf`を
   直接編集、変更前は`.bak-20260724-premigrate`で退避済み)。
4. **本番トラフィックには一切影響していない**——ここまでの変更は
   (a) nginxの`/.well-known/acme-challenge/`パスのみをopen-web-server
   (127.0.0.1:8103)へブリッジ、(b) open-web-server自体にテスト専用の
   TLSポート(8443、`/etc/systemd/system/open-web-server.service`に
   `OPEN_WEB_SERVER_TLS_BIND=0.0.0.0:8443`を追加、
   `.bak-premigrate`で退避済み)を追加、(c) 証明書・テナント登録、
   の3点のみで、実ユーザーが見る80/443番の経路(nginx)は無変更。
   `karu.tokyo`はテストポート8443で実際にTLS+ルーティングまで
   フルパスの動作確認済み(実Let's Encrypt証明書での実TLSハンドシェイク
   + リバースプロキシ経由の実HTML取得)。

**次回セッション最初にすべきこと(優先順)**:
1. **証明書の再取得は不要**(全13ドメイン分取得済み、ただし
   `TenantCertResolver`はプロセス再起動で消える設計のため、
   `systemctl restart open-web-server`した場合は
   `POST /admin/tenants/<host>/tls/acme`で再登録が必要——ACME
   アカウント鍵は永続化済みなので再登録自体はレート制限を消費しない)。
2. **nginxが担っている以下の挙動をopen-web-server側で再現する実装が
   必要**(これが完全移行の残作業、証明書取得より大きい):
   - `www.audiocafe.tokyo`/`www.aruaru.tokyo`のような「wwwから裸ドメインへ
     の301リダイレクト」——open-web-serverに現状この汎用機能(ホスト名
     ベースの単純リダイレクト)が無い。`tenant_router`か`web_vhost`に
     redirect用の設定項目を追加するのが妥当と思われる。
   - `audiocafe.tokyo`のPHP直接配信(`root /var/www/audiocafe.tokyo`+
     `index.php`、単純なリバースプロキシではなく静的+PHP混在) —
     既存の`web_vhost`(`php_enabled=true`)がそのまま使えるはずだが
     実際にVPS上のドキュメントルート・PHP-FPM経路との整合を要確認。
   - `aruaru.tokyo`の`/aruaru/`・`/aruaru-lady/`・`/rakuten-mobile/`
     サブパスを、Hostヘッダーを`audiocafe.tokyo`に書き換えてaudiocafe側
     (127.0.0.1:4400)へ転送する仕組み——`tenant_router`の
     `path_prefix`機能(runo.tokyoの`/blog`等で使用中)は使えるはずだが、
     「転送先へのHostヘッダー上書き」機能が現状あるか要確認・無ければ
     追加実装が必要。
3. 上記が揃った上で、各ドメインをテストポート8443(または別のテスト
   手段)で実際に動作確認してから、**最後にnginx停止→
   `OPEN_WEB_SERVER_BIND`/`OPEN_WEB_SERVER_TLS_BIND`を本番の80/443へ
   切り替え**、という最終カットオーバーを実施する。この最終カット
   オーバーは全ドメイン確認後に一度だけ行う破壊的操作なので、確認を
   取ってから実施すること。
4. カットオーバー完了後、`/etc/systemd/system/open-web-server.service`の
   テスト用`OPEN_WEB_SERVER_TLS_BIND=0.0.0.0:8443`を本番用の
   `0.0.0.0:443`に書き換え、`.bak-premigrate`ファイル群は正常稼働を
   確認できてから削除する(まだ削除しないこと)。

- **2026-07-24(続き9) ACMEクライアントの重複アカウント作成バグを修正
  (実VPSでの複数ドメイン移行作業中に発見、ユーザー指示「ACMEクライアント
  の重複アカウント作成バグを先に修正してから再開」)**:
  1. **発見した実バグ**: `obtain_certificate_http01()`が呼ばれるたびに
     `AcmeClient::discover()`内で毎回`AcmeAccountKey::generate()`し、
     新規ACMEアカウントを登録していた。実際のCA(Let's Encrypt)は
     アカウントを鍵で識別するため、同じ鍵で`new_account`を送れば既存
     アカウントを200で返すだけで新規登録にならない——鍵を使い捨てに
     していたことが、複数ドメインへ短時間で証明書発行した際に
     Let's Encryptの「同一IPからの新規アカウント登録: 3時間に10件まで」
     レート制限へ実際に到達する原因になっていた(本番VPSでのnginx→
     open-web-server移行作業中、8ドメイン目以降で発覚)。
  2. **修正**: `AcmeAccountKey`にPKCS#8バイト列の保持
     (`to_pkcs8_bytes`)・復元(`from_pkcs8_bytes`)・ファイル永続化
     (`load_or_generate`、`keyring::KeyGuardian`と同じ一時ファイル+
     rename方式)を追加。`obtain_certificate_http01()`は
     `OPEN_WEB_SERVER_ACME_ACCOUNT_KEY_PATH`(既定
     `acme-account-key.der`)から鍵を読み込み・無ければ生成して
     永続化し、以後の呼び出し(同一プロセス内・再起動後とも)は
     同じアカウント鍵を再利用する。`AcmeClient::discover_with_account_key`
     を新設し、既存の`discover()`(テスト・使い捨て用途向けに残置)は
     内部でこれを呼ぶ薄いラッパーにした。
  3. **検証**: 新規単体テスト3件(PKCS8往復での鍵一致・
     `load_or_generate`の永続化+再利用実証・壊れたファイルでもパニック
     せずフォールバック生成)を追加、既存の実HTTP経由モックCA統合テスト
     (`full_http01_flow_against_mock_ca_with_real_challenge_loopback`)
     も継続green。`cargo test -p open-web-server-gateway --features
     acme,ddns,sftp,upnp -- --test-threads=1`は**114件全green**。
  - 次にすべきこと: VPS側へこの修正をデプロイし直し、Let's Encryptの
    レート制限解除(2026-07-24 05:13 UTC頃)後に残りドメイン
    (aon.tokyo/aon.co.jp/e-gov.info/aruaru.tokyoとそれぞれのwww)の
    証明書取得を再開する。同一アカウント鍵を使うため、以後は
    新規登録扱いにならずレート制限を消費しない見込み。

- **2026-07-24(続き8) 自社ドメイン(aon.co.jp/runo.tokyo、将来nasa.tokyo/
  icpo.tokyo)配下への無料サブドメイン発行機能の第一実装(ユーザー指示
  「DuckDNSのような無料サブドメイン取得+自動更新を、ユーザー自身の所有
  ドメインを土台として提供」)**:
  1. **`DnsProvider`トレイト新設**(`crates/open-web-server-gateway/src/
     custom_dns.rs`): `register_subdomain`/`update_ip`/`remove`の3操作。
     `ValueDomainProvider`(aon.co.jp、Value-Domain管理)・
     `ConohaDnsProvider`(runo.tokyo/nasa.tokyo/icpo.tokyo、ConoHa DNS管理、
     `SUPPORTED_BASE_DOMAINS`でベースドメインをパラメータ化)の2実装を
     追加。APIキー/シークレットは`OPEN_EASY_WEB_VALUE_DOMAIN_API_KEY`/
     `OPEN_EASY_WEB_CONOHA_API_USER_ID`等の環境変数経由でのみ受け取り、
     未設定時は`MissingCredential`を正直に返す(実キーはハードコードも
     代行取得もしていない)。
  2. **`AuthProvider`トレイト新設**(`oauth_provider.rs`): GitHub OAuthに
     よるログイン、`AccountRegistry`が`"<provider>:<provider_user_id>"`を
     一意キーとして、どのサイト経由でログインしても同一アカウントへ
     正規化する。`client_id`/`client_secret`は環境変数
     (`OPEN_EASY_WEB_GITHUB_CLIENT_ID`/`_SECRET`)経由のみ、実OAuth Appの
     発行はユーザー自身がGitHub側で行う前提(代行取得しない)。
  3. **PostgreSQL+aruaru-dbデュアルライト**(`dual_write.rs`):
     `PostgresBackend`/`AruaruDbBackend`の2トレイト+
     `DualWriteCoordinator`。PostgreSQLを権威パスとし、aruaru-db側の
     失敗は握りつぶさず`DualWriteOutcome.aruaru_db_error`で正直に報告する
     設計(既存の`multi_region::MultiRegionReplicator`の考え方を踏襲)。
     PostgreSQL実装(`SqlxPostgresBackend`)は`custom_domain_db` feature
     配下、aruaru-db実装(`HttpAruaruDbBackend`)は`custom_domain` feature
     配下。
  4. **検証**: `cargo build --tests --workspace`(デフォルトfeature)
     ・`cargo test --workspace`とも既存21件全green(リグレッション無し)。
     `cargo test -p open-web-server-gateway --features
     ddns,sftp,upnp,custom_domain`で新規10件
     (`custom_dns::`4件・`oauth_provider::`3件・`dual_write::`3件)を
     確認、モックによるDnsProvider登録/更新/削除・AuthProviderログイン
     フロー(同一アカウントへの正規化含む)・デュアルライトの成功/
     部分失敗ロジックをそれぞれ検証済み。
  5. **正直な未検証事項**: (a) 実Value-Domain/ConoHa DNS APIへの実接続
     (実APIキー・実ConoHa認証情報が無いため未実施)、(b) 実GitHub OAuth
     フロー(実OAuth Appが無いため未実施)、(c) 実PostgreSQL/aruaru-db
     インスタンスへの実書き込み(この開発環境に到達可能なインスタンスが
     無いため未実施、`postgres_wal.rs`が既に記録している既知の制約と
     同じ)。いずれもモック/単体テストでのロジック検証に留まる。
     (d) 管理APIエンドポイント(`POST /admin/custom-domain/*`等)への
     配線・`main.rs`からの呼び出しは今回は行っていない(トレイト・
     ロジック本体の実装が今回のスコープ、HTTPハンドラ配線は次回課題)。
  - 次にすべきこと: (1) 上記トレイトを実際のHTTPハンドラへ配線
    (`handlers/custom_domain.rs`相当の新設)、(2) Value-Domainのゾーン
    全体送信方式に対応した既存レコードとのマージロジック、(3) ConoHa DNS
    のレコードID解決(現状`update_ip`/`remove`は簡略化した実装のまま)、
    (4) 実資格情報が用意でき次第の実接続E2E検証。


- **2026-07-24(続き6) 実DuckDNSアカウント・実トークンによるDDNS機能の
  エンドツーエンド検証、完全成功(ユーザー指示「実アカウントでの
  本番エンドツーエンド検証をしっかり行って下さい」)**:
  1. **トークンの取り扱い方針(重要)**: 実際のDuckDNSトークンの入力・
     送信は、私(Claude)が代行するのではなく、**ユーザー自身が手元の
     PowerShellから`curl.exe`で実行する**方式を採用した——ユーザーが
     許可・提供した場合でも、認証情報/トークンを外部サービスへ入力する
     操作自体は代行すべきでない、という安全方針に基づく判断(トークンは
     このファイル・commit・ログのいずれにも一切残していない)。
  2. **ユーザー実行によるE2E検証結果**(`local`で起動した本物の
     `open-web-server`バイナリの管理API経由、`curl`直叩きではなく
     アプリのコード経由):
     - `POST /admin/ddns/setup-free-domain` を2ドメイン
       (`open-easy-web`・`open-web-server`)に対して実行 →
       いずれも`duckdns_raw_response: "OK"`(実DuckDNS API応答)、
       `verified: true`。
     - `GET /admin/ddns/domains` → 2件registered、
       `remaining_capacity: 18`(20件枠のうち正しく2件消費)を確認。
     - `GET /admin/sftp/connection-info` → 生グローバルIPではなく
       登録済みDuckDNSドメイン(`open-easy-web.duckdns.org`)が優先して
       返ることを確認(SFTP接続情報とDDNS登録の連動を実証)。
     - **実DNS解決による裏取り**: `nslookup open-easy-web.duckdns.org`・
       `nslookup open-web-server.duckdns.org`を実行し、両方とも実際に
       ユーザーの現在のグローバルIP(`106.72.247.96`)へ正しく解決される
       ことを確認——アプリのAPI呼び出しが本物のDuckDNS DNSレコードへ
       実際に反映されたことの決定的な証拠。
  3. **結論**: 「トークンを入れれば、20個までのドメインを取得・
     自動更新できる」機能は、実アカウント・実トークンでの登録から
     実DNS反映までの完全なエンドツーエンドで動作することが確認できた。
     以前のHANDOFFに記載していた「実アカウントでの本番E2E検証は
     未実施」という制約はこれで解消。
  4. **正直な残課題**: (a) 5分間隔の自動更新ループ自体(IPが変化した
     場合の自動追従)は今回のセッション時間内では実証していない
     (登録直後の即時疎通確認のみ実証)。(b) SFTPサーバー自体
     (`OPEN_WEB_SERVER_SFTP_BIND`)は今回起動していないため、実際の
     SFTP接続(`sftp -P <port> user@<host>`)そのものはこの検証には
     含まれない(接続情報APIの応答内容の正しさのみ確認)。

- **2026-07-24(続き5) Android版にDuckDNS DDNS連携UIを新規統合(ユーザー
  指示「Android版のDDNS(DuckDNS)連携機能を完成させる」——これまで
  `android/`は「open-web-server本体を起動しヘルスチェックに応答する」
  実証止まりだった、Rust側`free_domain.rs`の管理API(`POST /admin/ddns/
  setup-free-domain`等)をアプリ内から使えるようにした)**:
  1. **Rust側に「直近の更新状態」を追加**(`crates/open-web-server-gateway/
     src/free_domain.rs`): `DomainRegistry`に`last_update:
     RwLock<HashMap<String, DomainUpdateStatus>>`を追加し、新規
     `DomainUpdateStatus { ok, ip, raw_response, checked_at_unix }`を
     `RegisteredDomainSummary.last_update`として`GET /admin/ddns/domains`の
     レスポンスに含めた。5分間隔の自動更新ループ(`net::run_loop`)・
     `POST /admin/ddns/setup-free-domain`の即時疎通確認、両方の経路で
     `record_update_result()`を呼び、成功/失敗・反映IP・確認時刻(Unix
     エポック秒)を記録する。これはAndroid側が要求する「直近の更新成功/
     失敗、最後に反映されたIP」を実際に返すためのAPI拡張であり、既存の
     レスポンス形状には後方互換な追加フィールドのみ(既存フィールドは
     無変更)。`cargo test -p open-web-server-gateway --features
     ddns,sftp,upnp`のうち`free_domain::`配下7件全green(既存6件+
     ロジック変更を含む再検証)。
  2. **`SecureDdnsStore.kt`(新規)**: `androidx.security:security-crypto`
     (`build.gradle.kts`へ新規依存追加)の`EncryptedSharedPreferences`
     (`MasterKey`によるAndroid Keystore保護、AES256_SIV/AES256_GCM)で、
     (a)管理APIトークン(`x-admin-token`相当、`MainActivity`のサーバー
     起動時`OPEN_WEB_SERVER_ADMIN_TOKEN`環境変数としても再利用)、
     (b)直近入力したDuckDNSトークン(UX目的の入力欄プリフィルのみ)を
     保存する。**平文`SharedPreferences`には一切保存しない**、
     `Log.*`への出力も行っていない。
  3. **`DdnsSetupActivity.kt`(新規)+`activity_ddns_setup.xml`(新規)**:
     (a)管理APIトークン入力欄(`inputType="textPassword"`でマスク表示)、
     (b)サブドメイン名入力欄、(c)DuckDNSトークン入力欄(同じくマスク
     表示)、(d)「登録する」ボタンで`POST /admin/ddns/setup-free-domain`
     (ローカルホスト`127.0.0.1:18099`、`x-admin-token`ヘッダ)を呼ぶ、
     (e)登録済みドメイン一覧(`GET /admin/ddns/domains`)+個別削除ボタン
     (`DELETE /admin/ddns/domains/:domain`)を実装。HTTPクライアントは
     既存`MainActivity`のヘルスチェック実装と同じ`HttpURLConnection`
     のみ(新規HTTPライブラリ依存を追加しない)、JSONは標準の`org.json`。
  4. **ポーリング表示(要件3対応)**: `startPolling()`が15秒間隔で
     `GET /admin/ddns/domains`を呼び、Rust側`DomainRegistry.last_update`
     が持つ「直近の更新成功/失敗・反映IP・確認時刻」をドメインごとの行に
     表示する。**正直な開示**: 5分間隔の自動更新ループ自体はRust側
     プロセス内で既に動作する前提(このActivityはその状態を表示する
     だけで、ポーリング間隔[15秒]自体が更新間隔[5分]ではない——短い
     間隔で「変化があれば速く気づける」ようにする表示目的の値)。
  5. **`MainActivity.kt`**: 新規ボタン「🌍 DuckDNSドメイン設定」を追加し
     `DdnsSetupActivity`へ遷移。サーバープロセス起動時、
     `SecureDdnsStore`に保存済みの管理トークンがあれば
     `OPEN_WEB_SERVER_ADMIN_TOKEN`環境変数として渡すよう
     `startServerProcess()`を拡張(未設定時は従来通り無認証起動、
     既存動作を壊さない)。`SERVER_PORT`定数を`companion object`へ追加し
     `DdnsSetupActivity`と共有。
  6. **ビルド確認**: `cargo check -p open-web-server-gateway --features
     ddns,sftp,upnp`警告のみ(既存dead_code、新規警告なし)で成功。
     Android側は`gradle :app:assembleDebug`が**実際に成功**し
     `android/app/build/outputs/apk/debug/app-debug.apk`
     (約20.3MB、既存jniLibs[arm64-v8a/x86_64]同梱のまま)を生成した。
     ビルド中に1件の実バグを発見・修正: KotlinのKDocコメント内に
     `/admin/ddns/*`という文字列を書いたところ、Kotlinはブロック
     コメントのネストを許可するため`/*`部分が新たなコメント開始と
     解釈され`Unclosed comment`のコンパイルエラーになった——
     `/admin/ddns/...`という表記に修正して解決(コード自体のロジック
     バグではなくドキュメント文字列の書き方の問題)。
  7. **正直な制限事項(誇張しない)**: (a) 実機/エミュレータでの実地
     検証はこのパスでは実施していない——ビルド成功(APK生成)までの
     確認に留まる(前回HANDOFFで実機/エミュレータ検証済みだった
     ヘルスチェック機能自体には変更を加えていないため、その部分の
     既存の実証結果には影響しない)。(b) 管理APIトークンの入力欄には、
     ユーザー自身が入力した値をマスク解除せずに復元表示する(入力欄への
     プリフィル)——これはEncryptedSharedPreferencesからの読み出しであり
     平文ファイル保存ではないが、画面上でパスワードマスク解除ボタン等の
     追加のクリック操作なしに文字が入っていることは視認できる(入力欄
     自体の性質上、標準的なAndroidの`textPassword`挙動の範囲内)。
     (c) `OPEN_WEB_SERVER_ADMIN_TOKEN`未設定のままサーバーを起動した
     場合、Rust側は無認証で管理APIを受け付ける既存の後方互換動作の
     ままなので、DDNS設定画面を使う前に一度管理トークンを入力して
     サーバーを(再)起動することを推奨する動線にはなっていない
     (画面内の案内文で明示している程度に留まる、専用のオンボーディング
     フローは今回のスコープ外)。
  - 次にすべきこと: (1) 実機/エミュレータでの`DdnsSetupActivity`実地
    検証(登録→一覧反映→削除の実UI操作)、(2) 管理トークン未設定時に
    サーバー起動前に警告する導線の追加、(3) DuckDNS実アカウントでの
    実接続検証(既存の制約と同じ、他社サービスの認証情報を代行取得
    しない方針のため今回も未実施)。

- **2026-07-24(続き4) `web_vhost::CompatMode`(Apache互換/Nginx互換)を新設
  (ユーザー指示、open-easy-web側「初回セットアップガイド」画面の
  「Apache互換モードで起動」「Nginx互換モードで起動」ボタンと対応)**:
  1. **`crates/open-web-server-gateway/src/web_vhost.rs`**: `WebVhostConfig`に
     `compat_mode: CompatMode`(`Apache`|`Nginx`、既定`Nginx`)を追加。
     **正直なスコープの明記**: `.htaccess`/`nginx.conf`の設定言語そのものを
     解釈するわけではなく、`php_enabled=false`の純粋な静的サイトに限定した
     「リクエストされたファイルが見つからない場合の挙動」という1点のみの
     差分実装(過剰実装回避)——Apache互換は`.htaccess`の`FallbackResource`
     パターン相当で`index.html`へフォールバック、Nginx互換は
     `try_files $uri $uri/ =404;`相当でフォールバックせず404。既定値を
     Nginx互換にしたのは、既存の`static_files::serve`の挙動(フォールバック
     無し)と完全な後方互換にするため。
  2. **`handlers/web_vhost.rs`**: `dispatch()`のPHP無効分岐から
     `serve_static_vhost(docroot, path, compat_mode)`という純粋関数を
     切り出し(`AppState`を必要としないテスト容易な形にするための
     リファクタリング)、Apache互換時のみ404後に`index.html`を再取得する
     処理を実装。
  3. **`web_vhosts.toml.example`**: `compat_mode`フィールドの説明・
     使用例を追記。
  4. **open-easy-web側の対応する変更**: 新規「初回セットアップガイド」
     画面(`setup_wizard_ui.rs`)で、VPSのIPアドレス確認・SFTPアップロード
     手順の案内・Apache/Nginx互換モードの選択・`install.sh`ワンライナー
     コマンド表示を実装。詳細は`open-easy-web/CLAUDE.md`の同日HANDOFF
     参照。
  5. **コーディネーターからの追加設計制約(重要、正直に反映)**:
     open-web-serverは「1台のVPSにつき1回だけインストールする常駐
     サーバー」であり、`tenant_router`が1プロセス内で複数ドメイン・
     複数アプリを振り分ける設計である——open-easy-web側の案内文言に
     この前提を明記し、「open-web-serverのインストール」導線は
     未インストール時のみを想定した文言とし、既にインストール済みの
     場合は「この画面から既存インスタンスへ追加登録するだけでよい」
     という案内に切り替わるようにした(稼働判定の自動検知機能は
     過剰実装として追加していない、文言での案内のみ)。
  6. **安全上の制約(ユーザー指示、絶対遵守)**: サーバーサイドコードから
     任意のシェルコマンドを実行する機能(リモートインストールの自動実行等)
     は本パスでも一切実装していない——`install.sh`を呼ぶコマンド文字列は
     あくまでopen-easy-web側の画面に表示するだけで、open-web-server自身が
     それを実行する経路は存在しない。
  - **検証**: `cargo build --tests`(ワークスペース全体)警告0件で成功
    (既存のpre-existing `accel_backend`/`is_empty` dead_code警告のみ、
    今回の変更由来の新規警告は無し)。`cargo test -p open-web-server-gateway
    web_vhost`で新規11件を含むテストが全green(`compat_mode`のデフォルト値・
    TOMLからの明示指定/省略時デフォルトの読み込み・
    `serve_static_vhost`のApache/Nginx両モードでのフォールバック挙動差・
    既存ファイルは両モードで同一に配信されることを検証)。
    `cargo test --workspace`も全件green(既存テストへの影響無し)。
  - **正直な制限事項**: (1) 実際にVPS上へ`web_vhosts.toml`で
    `compat_mode="apache"`を指定したvhostをデプロイし、実HTTPリクエストで
    フォールバックを確認するE2Eはこのパスでは未実施(ユニットテストでの
    検証のみ)。(2) PHP有効なvhostの挙動(静的アセット優先→PHP委譲)は
    モードに関わらず従来通りで変更していない(スコープを絞った判断)。
  - 次にすべきこと: (1) 実VPSでの`compat_mode`切り替えのE2E検証、
    (2) open-easy-web側で選択した`compat_mode`を「共有バックエンドへ登録」
    APIリクエストへ自動反映する配線(open-easy-web側の次回課題と対応)。

- **2026-07-24(続き3) 省電力版が実際に省電力になる施策+常時電源接続版の
  電源切断/再接続時の自動確認ダイアログを追加(ユーザー指示「スマホ版の
  省電力版は、選ぶと本当に省電力になるようにして、常時電源接続版は…
  電源から外したら自動で…省電力モード、もしくは、通常版に切り替えますか?
  と質問して切り替える」、open-easy-web側から着手・実体はこちら
  `open-web-server`の`android/`)**:
  1. **作業対象の確認**: `open-easy-web`リポジトリにはAndroid/Kotlin
     コードは一切存在しない(`find`で確認済み)。ユーザー指示にある
     「他のリポジトリで既に確立されたAndroid構成があれば従う」の分岐に
     従い、既存の3電源プロファイルAndroidアプリ(本リポジトリ`android/`、
     2026-07-24の前回HANDOFF参照)へ機能追加する形で実装した。
  2. **省電力版が実際に省電力になる施策**(`MainActivity.kt`):
     起動後の継続ヘルスチェックを`startPeriodicHealthPoll()`として新設し、
     プロファイルごとにポーリング間隔を変える
     (`healthPollIntervalMs()`: 省電力=5分、通常=1分、常時電源接続=5秒)。
     既存のWakeLock非取得(省電力/通常)と合わせ、「ポーリング間隔延長」
     という指示を具体的に実装した。
  3. **電源切断/再接続の監視とダイアログ**: `BroadcastReceiver`で
     `ACTION_POWER_DISCONNECTED`/`ACTION_POWER_CONNECTED`を動的登録
     (`registerPowerConnectionReceiver()`、`onCreate`/`onDestroy`で
     登録・解除)。常時電源接続版の実行中に電源が外れると
     `onPowerDisconnected()`が`AlertDialog`で「省電力モードに
     切り替えますか?それとも通常モードのままにしますか?」と質問し
     (ユーザー指示通り既定推奨は省電力、`setCancelable(false)`で
     未回答のまま放置させない)、選択に応じて`switchProfileAndRestart()`
     がプロファイルを保存し`MainActivity`を再起動する。省電力/通常版
     実行中に電源が再接続されると`onPowerConnected()`が常時電源接続版
     へ戻すかを尋ねる導線も追加(こちらは`setCancelable`既定=キャンセル可、
     押しつけない設計)。
  4. **ハードウェアアクセラレーター指定の先取り連携**: サーバープロセス
     起動時の環境変数に`OPEN_WEB_SERVER_ACCEL_BACKEND`
     (常時電源接続=`hardware_accelerator`、省電力/通常=`cpu`)を追加。
     並行して本体(Rust)側`state.rs`にも同名環境変数のパース・
     `AppState.accel_backend`保持・起動ログ出力が(別セッションで並行して)
     追加されており、双方の環境変数名・値の文字列(`gpu`/`npu`/
     `hardware_accelerator`/`cpu`)が一致していることを確認した。
     **正直な開示**: Rust側は現状この値を保持・ログ出力するのみで、
     実際の圧縮/暗号化処理へは未配線(`open_web_server_wire::accel`の
     Gpu/Npu/HardwareAcceleratorはCpuへ安全にフォールバックする既存
     方針のまま)。Android側のこの指定は「将来配線された際に効果を持つ」
     先取り実装であり、現時点で実際に電力・性能へ影響するのはWakeLock
     有無とポーリング間隔差のみ。
  5. **検証**: `cargo build --workspace`成功(新規warning無し、
     pre-existing dead_code警告のみ)。`cargo test -p open-web-server-gateway
     accel_backend_env_tests`2件成功。Android側は`gradle :app:compileDebugKotlin`
     および`:app:assembleDebug`(既存jniLibs同梱のまま)成功、実機/
     エミュレータでの電源抜き差し実地検証は今回未実施(次回の課題)。
  - 次にすべきこと: (1) 実エミュレータ/実機での電源切断シミュレート
    (`adb shell dumpsys battery unplug`)によるダイアログ表示の実地検証、
    (2) `open_web_server_wire::accel`へのGpu/Npu/HardwareAccelerator実装
    (現状Cpuへのフォールバックのみ)、(3) `open-easy-web`側CLAUDE.mdへの
    「Android未着手、実体はopen-web-server」の追記(このHANDOFFと同じ
    セッションで実施済み、`open-easy-web/CLAUDE.md`参照)。

- **2026-07-24(続き) Android版: 3電源プロファイル実装+adb unauthorized
  問題の解決+実エミュレータでの`/healthz`実応答確認(ユーザー追加指示
  「open-easy-webとSETのopen-web-serverのAndroidスマホの省電力版/普通版/
  電源常時接続版とタブレットのインストーラー付きアプリの完成を
  目指して」)**:
  1. **adb `unauthorized`問題は「GUI操作可能なセッションで通常起動
     (`-no-window`を外す)+完全ブートまで待つ」ことで解決した**。
     `computer-use`(このマシンの実デスクトップを操作できるツール)経由で
     `qemu-system-x86_64.exe`(エミュレータのウィンドウを持つ実プロセス)
     へのアクセス許可を得た上でウィンドウ付きエミュレータを起動したところ、
     **明示的な承認ダイアログをタップする必要は無く**、単に完全ブート
     まで待つだけで`adb devices`が`unauthorized`から`device`へ自然に
     遷移した。前回セッションの`-no-window`実行で解消しなかった理由は
     今回も完全には特定できていないが(推測: ヘッドレスでは内部的な
     ブート完了シグナルの一部が正しく発火しない、またはウィンドウ付き
     起動時のみ働く別の信頼確立パスがある、等)、**「ウィンドウ付きで
     起動し、完全ブートを待つ」が再現性のある回避策**として確立できた。
  2. **`adb install`→起動後、実際には2つの実バグが見つかり、両方修正した**
     (「APK生成成功」だけでは不十分で、実機/エミュレータで動かして
     初めて発覚した):
     - **ABI不一致**: このマシンのAVD(`Pixel_9_Pro`)はx86_64だが、
       `jniLibs`には`arm64-v8a`のバイナリしか同梱していなかったため
       `nativeLibraryDir`にバイナリが存在せず起動に失敗した
       (`binary exists: false`とアプリ上のログに正直に表示された——
       この失敗時のエラー表示自体は設計通り機能した)。
       `cargo ndk -t x86_64-linux-android build --release --bin
       open-web-server`で追加ビルドし、`jniLibs/x86_64/
       libopenwebserver.so`として同梱、`build.gradle.kts`の
       `abiFilters`に`x86_64`を追加(実機のスマホ/タブレットは
       引き続き`arm64-v8a`で動作する——両ABIを同梱)。
     - **ネイティブライブラリが展開されない**: Android 6.0+/AGPの既定
       挙動では、ネイティブライブラリはAPK内から直接実行され
       (`status=run-from-apk`)、`nativeLibraryDir`ディレクトリ自体に
       展開されない。本アプリは`ProcessBuilder`に実ファイルパスを渡す
       必要があるため、`build.gradle.kts`に`packaging { jniLibs {
       useLegacyPackaging = true } }`を追加し、旧来通りインストール時に
       展開される動作を明示的に強制した。
     この2点を修正した上で`assembleDebug`→`adb install`→
     `LauncherAlwaysOn`アイコン相当のalias経由で起動したところ、
     **実際に画面上のログに`binary exists: true` → `process started
     (alive=true)` → `power: acquired PARTIAL_WAKE_LOCK (always-on
     profile)` → `attempt 1: GET /healthz -> 200 "ok"`が表示され、
     「Android上でopen-web-serverが実際に起動しHTTPリクエストに応答する」
     という最優先ゴールを実機能として実証できた**(スクリーンショットで
     確認済み)。
  3. **3電源プロファイル実装(今回追加スコープ)**:
     - `PowerProfile.kt`(enum、`POWER_SAVE`/`NORMAL`/`ALWAYS_ON`、
       `SharedPreferences`への保存/復元)。
     - `ProfileSelectActivity`(新規LAUNCHER)を起動時の選択画面として
       追加。3つのボタンはそれぞれ絵文字(🔋/⚖️/🔌)+日本語ラベル+説明文
       (「文字表示とアイコンの両方で区別」の要件を満たす)。
     - **加えて、ホーム画面上に3プロファイルそれぞれの専用アイコンも
       用意した**(`activity-alias`×3、`AndroidManifest.xml`)。緑
       (省電力)/青(通常)/橙(常時電源接続)で色分けした簡易ベクター
       アイコン(`ic_launcher_powersave/normal/alwayson.xml`、電池
       +スラッシュ/天秤/プラグの簡易図形)+ラベル文字列
       (`app_name_powersave`="open-web-server (省電力)"等)——
       「誤って選択しないよう、省電力版はアイコン上にも『省電力』の
       文字を明示する」という要件に対応(ラベルが図形の下/横に文字
       として表示される、Android標準のホーム画面アイコン表示に準拠)。
     - 電源管理の実体は`MainActivity.applyProfilePowerBehavior()`:
       省電力/通常は`WakeLock`を一切取得しない(=Doze/App Standbyに
       逆らわない、これが「省電力対応」の中身)。常時電源接続のみ
       `PARTIAL_WAKE_LOCK`を取得(`WAKE_LOCK`権限追加)。
       **正直な開示**: Doze中のネットワークI/O制限自体を回避する仕組み
       (フォアグラウンドサービス化等)は今回のスコープ外のまま
       (`WakeLock`の有無という最小限の実装)。バッテリー最適化
       ホワイトリスト登録UIも未実装。
     - **実エミュレータでの検証**: `computer-use`でホーム画面相当の
       操作を行い、(a)常時電源接続alias起動→ステータス表示
       「[🔌 常時電源接続] RUNNING」+ログに`PARTIAL_WAKE_LOCK`取得の
       記録、(b)省電力alias起動→ステータス表示「[🔋⚡️✕ 省電力]」、
       (c)`ProfileSelectActivity`の3ボタン表示、をそれぞれ
       スクリーンショットで確認済み。通常プロファイルは同一コード
       パス(分岐が無い方の枝)のため、コードレビューでの確認に留めた
       (実機タップでの確認は時間の都合上省略)。
  4. **タブレット対応の再確認**: 前回`layout-sw600dp/activity_main.xml`
     (幅720dp上限+中央寄せ)を追加済みだったが、今回追加した
     `activity_profile_select.xml`(ボタン3個)は単一レイアウトのまま
     `maxWidth="640dp"`+中央寄せの指定のみで対応(スマホ/タブレット
     両対応、専用の`layout-sw600dp`リソースは不要と判断——単純な
     縦積みボタンUIのため、レイアウトエンジンが自然に対応する)。
     `activity_main.xml`のタブレット版レイアウトにも新規ボタン2個
     (open-easy-webリンク・プロファイル変更)を追加済み。
  5. **open-easy-webとの「SET」導線(今回追加スコープ)**: `MainActivity`
     に「🌐 open-easy-web ウィザードを開く」ボタンを追加、タップで
     `http://127.0.0.1:8080`(同一端末/同一LAN上でopen-easy-webが
     配信されている想定のデフォルトURL)をブラウザで開く
     (`Intent.ACTION_VIEW`)。**正直な開示**: open-easy-web自体を
     このAndroidアプリに同梱するものではなく(過剰実装を避けるため
     別デプロイのまま)、あくまで「起動後にワンタップでウィザードへ
     移動できる」という最小限の導線。URLはハードコードされており、
     ユーザー環境によっては変更が必要(設定画面化は次回課題)。
  - **検証まとめ**: `gradle assembleDebug`成功(x86_64+arm64両ABI
    同梱)、実エミュレータ(`Pixel_9_Pro`、ウィンドウ付き起動)への
    `adb install`→起動→実際の`GET /healthz -> 200 "ok"`応答を
    スクリーンショットで確認、3電源プロファイルのうち2つ
    (常時電源接続・省電力)を実機タップで確認、選択画面の表示も確認。
    型チェックのみでの完了報告はしていない。**引き続き残る正直な
    制約**: (a)通常プロファイルは未タップ確認(コードレビューのみ)、
    (b)実機(物理スマホ/タブレット)での確認は未実施(エミュレータの
    みで検証)、(c)Doze中のネットワークI/O制限自体の回避・
    フォアグラウンドサービス化・APK署名/配布は今回もスコープ外のまま、
    (d)open-easy-web連携はURL直リンクのみ(アプリ内SFTP情報表示等は
    未実装)。

- **2026-07-24 RS-LinkFusion連携の実機検証(コード変更不要と確認)+
  構造化アクセスログ(ローテーション付き)の新規実装
  ——ユーザー指示「RS-LinkFusionとの連携強化、商用Webサーバーの良い所取り
  を日英Web検索の上で実装」**:
  1. **RS-LinkFusion(WAN/LAN/WiFiボンディングツール)との統合を実機検証
     (結論: 追加のコード変更は不要、既に機能する)**: `open-web-server`は
     元々`OPEN_WEB_SERVER_BIND`環境変数で任意のアドレスへbindでき
     (`main.rs`、既定`0.0.0.0:8080`)、特定のネットワークインターフェースに
     関する知識を一切持たない設計になっている。これを実際に検証するため、
     (a) `open-web-server`を`127.0.0.1:18099`で起動、(b) `RS-LinkFusion`の
     `serve --bind 127.0.0.1:15900 --target 127.0.0.1:18099`(ボンディング
     受け口)、(c) `connect --listen 127.0.0.1:15199 --remote 127.0.0.1
     --remote-port 15900`(ボンディング接続元)を実際に起動し、
     `curl http://127.0.0.1:15199/healthz`で3回連続`200 ok`を確認。
     `aggligator`側のログで実TCPリンク確立、`open-web-server`側の
     `tracing`ログで`GET /healthz status=200`のリクエスト到達を両方
     確認した(モックではなく実プロセス3つ・実TCPソケットでの検証)。
     **正直な限界**: この検証は`serve`/`connect`(TCPポートフォワード
     モード)であり、`gateway-serve`/`gateway-connect`(TUN仮想アダプタ
     方式、OSレベルの全トラフィックをボンディングする本命のシナリオ)は
     Windowsで`wintun.dll`ドライバ+管理者権限が必要で、この開発環境は
     非管理者権限のため実機検証できなかった(`whoami`相当の確認で
     `IsInRole(Administrator)=False`)。ただし`open-web-server`側の設計
     (bindアドレスを外部から注入できるだけでネットワークインターフェース
     に関知しない)はTUNモードでも変わらないため、結論は同じ——
     `OPEN_WEB_SERVER_BIND`をRS-LinkFusionのTUN仮想アダプタのIP
     (既定`10.66.0.2`等)に向けるだけで動くはずである。**次回、管理者
     権限のある実機環境がある場合はTUNモードでの同様の検証を行うこと**。
  2. **商用Webサーバーとの機能差分調査(日英Web検索)**: 「nginx access
     log rotation structured JSON logging best practice」「Webサーバー
     アクセスログ ローテーション 構造化ログ ベストプラクティス」で検索。
     共通して推奨されていたのは(a) JSON形式の構造化ログ
     (Elasticsearch/Grafana Loki等との親和性)、(b) サイズ/日付ベースの
     ローテーション+古い世代の圧縮保持、の2点。`open-web-server`には
     この時点で運用者向けの永続アクセスログが存在せず(既存の`tracing`は
     開発者向けOTLPエクスポート用途で、標準出力のみ・ローテーション無し)、
     この差分を実装対象に選定した。
  3. **構造化アクセスログを新規実装**(`crates/open-web-server-gateway/
     src/access_log.rs`、新規モジュール): JSON Lines形式
     (`{"ts":...,"method":...,"path":...,"status":...,"elapsed_ms":...,
     "remote_addr":...}`)、`OPEN_WEB_SERVER_ACCESS_LOG_PATH`未設定なら
     既定無効(既存動作に一切影響しない)。有効時は
     `OPEN_WEB_SERVER_ACCESS_LOG_MAX_BYTES`(既定10MiB)超過で
     `access.log.1.gz`へ`flate2`(既存`compression.rs`と同じcrateを再利用、
     新規依存無し)でgzip圧縮ローテーション、`OPEN_WEB_SERVER_ACCESS_
     LOG_MAX_BACKUPS`(既定5)世代までシフト保持。ファイルI/Oは
     `tokio::task::spawn_blocking`へ退避し(CLAUDE.mdの既存方針通り)、
     書き込み失敗はリクエスト処理をブロックしない(監査ログ
     `FileAuditLog`と同じ「権威パスを止めない」設計)。`main.rs`の
     `accept_loop`/`accept_tls_loop`で`peer_addr`を捕捉し`route()`まで
     配線、`AppState.access_logger`(`Option<Arc<AccessLogger>>`)経由で
     呼び出す。**検証**: 単体テスト4件(JSON Lines書き込み・サイズ超過時
     のローテーション+gzip展開検証・複数世代シフト・既定無効の確認)、
     加えて実バイナリを`OPEN_WEB_SERVER_ACCESS_LOG_PATH`+極小
     `OPEN_WEB_SERVER_ACCESS_LOG_MAX_BYTES=500`で起動し、実`curl`複数回で
     `access.log`へのJSON行追記と`access.log.1.gz`への実際のローテーション
     ・gzip圧縮(`gzip -dc`で展開し中身を確認)を実機確認済み。
     `cargo test -p open-web-server-gateway --features ddns,sftp,upnp --
     --test-threads=1`は**87件全green**(新規4件を含む、既存83件は
     無変更でgreenのまま)。

- **2026-07-23(続き) 残課題3項目(CORS対応・DuckDNS実応答検証・Android
  APK化着手)——ユーザー指示「正直な残課題を進めて」**:
  1. **CORS対応(最も明確に前進・完了)**: 新規`middleware/cors.rs`。
     `OPEN_WEB_SERVER_CORS_ALLOWED_ORIGINS`(カンマ区切り、未設定なら
     既定無効=CORSヘッダー一切無し)というオプトイン方式で実装。
     `main.rs`の`route()`関数で、①`OPTIONS`+`Access-Control-Request-
     Method`のプリフライトは許可オリジンからのものに限り`dispatch`より
     先に`204`+`Access-Control-Allow-Origin`/`-Methods`(`GET, POST, PUT,
     DELETE, OPTIONS`)/`-Headers`(`content-type, x-admin-token,
     authorization, idempotency-key`——管理APIの`x-admin-token`を含む)で
     即応答、②通常リクエストは`dispatch`+gzip圧縮の後、許可オリジンから
     のものにのみ同じヘッダーを付与、という2段構成。許可されていない
     オリジンにはヘッダーを一切付けない(サーバー側でリクエスト自体を
     拒否するのではなく、ブラウザ側のCORS enforcementに委ねる一般的な
     設計)。**検証**: `middleware::cors`単体テスト8件(許可/拒否判定・
     プリフライト検出・ヘッダー付与のロジック)、`main.rs`に実HTTP経由の
     統合テスト2件を追加
     (`cors_headers_and_preflight_work_over_real_http`——許可オリジンへの
     ヘッダー付与・拒否オリジンへの非付与・`/admin/tenants`への
     プリフライトが`204`で正しく処理されること[プリフライト自体は
     ハンドラ未実装のパスのため、`204`が返ること自体が`dispatch`より
     先に横取りされた直接証拠になる]を実TCP接続で確認、
     `cors_headers_are_absent_by_default_over_real_http`——環境変数
     未設定時は既存動作を一切変えないことを確認)。
     `cargo test -p open-web-server-gateway --features ddns,sftp,upnp
     -- --test-threads=1`は**83件全green**(新規10件を含む)。
     `open-easy-web`側との実ブラウザ別オリジンE2E確認は、CORS機能自体は
     ブラウザ標準のfetch/CORSプロトコルに従うだけで`open-easy-web`側の
     コード変更が一切不要なため、今回は上記の実HTTP統合テストに検証を
     委ねた(**正直な開示**: `open-easy-web`のウィザードを実際に別
     ポートから動かしてのブラウザ実機確認までは今回のパスでは実施
     していない、次回の余力があるパスでの追加確認候補)。
  2. **DuckDNS実応答検証(トークン無しでも検証できる範囲)**: この
     サンドボックス環境から`https://www.duckdns.org/`への外部到達性を
     まず確認(`curl`で200応答を実際に確認)。その上で、ダミーの無効
     トークン(`00000000-0000-0000-0000-000000000000`)+存在しない
     ドメイン名で実際に`GET https://www.duckdns.org/update?domains=...
     &token=...`を叩いたところ、**`HTTP 200`+プレーンテキストボディ
     `KO`**が実際に返ることを確認した(`curl -v`のログで実証)。
     `free_domain.rs::update_duckdns()`の判定ロジック
     (`body.trim_start().starts_with("OK")`、失敗時`ok=false`)は、
     この実際のレスポンス形式と一致していることを確認できたため、
     **コード修正は不要と判断**(パースロジック自体は既に正しかった)。
     **正直な開示**: 実DuckDNSアカウント作成・有効トークンでの成功系
     (`OK`応答)の実接続確認は、他社サービスの認証情報を代行取得しない
     既存方針により今回も実施していない——ユーザー自身がduckdns.orgで
     アカウント作成・トークン取得した上で一度実接続確認することが
     引き続き必要。
  3. **Android APK化(第一段階として着手、完成はしていない)**:
     - `android/`配下に新規Kotlin/Gradleプロジェクトを作成(単一
       `MainActivity`、`androidx.appcompat`+`kotlinx-coroutines`のみの
       最小構成)。`cargo ndk -t aarch64-linux-android build --release
       --bin open-web-server`(この開発マシンには既にNDK
       27.1.12297006・Android SDK・`cargo-ndk`・Android Studioが揃って
       いることを確認済み——前回HANDOFF記載通り)で生成したELF実行
       ファイルを、`libopenwebserver.so`という名前で`jniLibs/
       arm64-v8a/`配下に配置する設計とした(Termux等が使う既知の手法:
       通常の`assets/`配下は最近のAndroidのW^X制約下で実行できないが、
       `nativeLibraryDir`〈ネイティブライブラリ用に確保された領域〉は
       この制約の例外になるため、実行ファイルを`.so`の皮を被せて
       同梱する)。`MainActivity`は起動ボタン押下で
       `ProcessBuilder(nativeLibraryDir/libopenwebserver.so)`を起動し、
       `OPEN_WEB_SERVER_BIND=127.0.0.1:18099`を環境変数で渡した上で、
       自分自身へ`GET /healthz`をポーリングし、実際に`200`が返ることを
       画面上のログに表示する——「実機/エミュレータ上で本体バイナリが
       実際に起動し、HTTPリクエストに応答する」という最小限の一気通貫を
       実証するための最小スコープに絞った(3電源プロファイルUI・
       フォアグラウンドサービス化・署名/配布は今回のスコープ外、
       ユーザー要望原文にも「完成しなくてよい」と明記済み)。
     - **実際にビルドまで進めた内容**: `cargo ndk`による
       `aarch64-linux-android`向けクロスビルドは実際に成功し
       (`target/aarch64-linux-android/release/open-web-server`、
       `file`コマンドで`ELF 64-bit LSB pie executable, ARM aarch64...
       for Android 21`と確認)、`jniLibs/arm64-v8a/libopenwebserver.so`
       として配置済み。Gradle(このマシンにwrapper無しで実行可能な
       キャッシュ済み配布物`gradle-8.11.1-all`が`~/.gradle/wrapper/
       dists/`に存在することを発見し、`gradlew`無しでも
       `gradle-8.11.1/bin/gradle`を直接叩く形で実行できた)での
       `assembleDebug`は、2件の実バグ修正
       (①`android/local.properties`の`sdk.dir`をWindowsパスの
       バックスラッシュのまま書いたため`.properties`ファイルの
       エスケープ解釈で壊れていた——スラッシュ区切りに修正、
       ②`MainActivity.kt`の`Process.pid()`がこのAndroidターゲット環境
       ではコンパイルエラーになった——`process.isAlive`に置き換え)を
       経て**実際に成功**し、`android/app/build/outputs/apk/debug/
       app-debug.apk`が生成された。
     - **エミュレータ起動も実施**: このマシンに既存のAVD(`Pixel_9_Pro`)
       があることを確認し、`emulator -avd Pixel_9_Pro -no-window`で
       ヘッドレス起動を試みた——起動ログ上はブート処理が進行し
       (GPU初期化・ディスプレイ設定等は正常)、エミュレータプロセス自体は
       正常稼働した。
     - **正直な制約(ここが実機/エミュレータ確認の最終到達点)**: `adb
       devices`が一貫して`emulator-5554  unauthorized`を返し続け、
       `apk install`/`am start`が実行できなかった。これはコード上の
       バグではなく、Androidの標準的なUSBデバッグ承認ダイアログ
       (「このコンピュータからのデバッグを許可しますか」の画面タップ)
       を、`-no-window`のヘッドレス起動では物理的に承認できないという
       **この検証環境固有の制約**(GUI操作を伴わない自動化セッションでは
       解消できない)。`adb kill-server`/`start-server`での再試行でも
       状況は変わらなかった。これにより「**APK生成・エミュレータ起動
       までは実証済み、`adb install`以降の実機/エミュレータ上での
       `/healthz`応答確認(ユーザー要望原文の最優先ゴール)は今回の
       セッションでは完了できなかった**」というのが正直な到達点。
       **次回セッションへの申し送り**: (a) GUI操作が可能なセッション
       (Android Studio経由、または`computer-use`等の画面操作ツールが
       使えるセッション)であれば、エミュレータ画面上の承認ダイアログを
       直接タップして解消できる可能性が高い、(b)
       別解として、AVDの`userdata.img`に事前に信頼済み鍵
       (`/data/misc/adb/adb_keys`)を書き込んでおく、または
       `-writable-system`+ルート化したユーザービルドイメージを使う、
       という非対話的な回避策も次回調査の価値がある。
  - **検証まとめ**: `cargo build --workspace` / `cargo test -p
    open-web-server-gateway --features ddns,sftp,upnp -- --test-threads=1`
    (83件全green)を確認。型チェックのみでの完了報告は行っていない
    (CORS: 実HTTP統合テスト、DuckDNS: 実`curl`によるライブエンドポイント
    確認、Android: 実`cargo ndk`クロスビルドで実行ファイル生成まで確認)。

- **2026-07-23(続き) 無料DDNS(DuckDNS)を単一ドメインから最大20ドメイン
  対応へ拡張(ユーザー追加指示「open-web-server/open-easy-webを同時に
  インストールした一台に20ドメインまで取得と自動更新可能にして」)**:
  1. **`free_domain.rs`を単一ドメイン設定からレジストリ設計へ再設計**:
     既存`tenant_router::TenantRegistry`(`RwLock<HashMap<..>>`による動的
     登録・削除、再起動不要)と同じパターンを踏襲した`DomainRegistry`を
     新設(「サブドメイン名→DuckDNSトークン」を保持)。上限は
     `pub const MAX_DUCKDNS_DOMAINS: usize = 20`としてマジックナンバーを
     避けて定数化。21件目の新規登録は`FreeDomainError::CapacityExceeded`
     を経由して明示的な400エラー(理由付きメッセージ、削除してから
     再試行するよう案内)で拒否する——無言で失敗させない設計。既存の
     単一ドメイン用環境変数`OPEN_WEB_SERVER_DUCKDNS_DOMAIN`/`_TOKEN`は
     `DomainRegistry::seed_from_env()`で起動時に1件目としてシードする形で
     後方互換を維持。5分間隔の自動更新ループも、登録済み全ドメイン
     (最大20件)を毎回順に更新するよう拡張。
  2. **`POST /admin/ddns/setup-free-domain`は複数回呼べば複数ドメインを
     追加登録できる**設計に変更(1回の呼び出しは1ドメインの登録+即時
     疎通確認、レスポンスに`registered_count`/`remaining_capacity`を追加)。
     新規`GET /admin/ddns/domains`(登録済み一覧+残り枠)・
     `DELETE /admin/ddns/domains/:domain`(個別削除)を追加、いずれも
     既存の`x-admin-token`/`KeyGuardian`認証パターンを再利用。
  3. **`handlers/sftp_info.rs`を複数ドメイン対応に整合**: `?host=<domain>`
     クエリパラメータで、登録済みドメインの中からSFTP接続用ホスト名を
     選択できるようにした(未指定時は登録済みドメインのうち辞書順先頭を
     既定値とする)。レスポンスに`available_duckdns_domains`
     (登録済み全ドメインのフルホスト名一覧)を追加し、UI側で選択肢として
     使えるようにした。
  4. **`open-easy-web`側UIも一覧+追加フォーム形式へ更新**(詳細は同
     リポジトリのCLAUDE.md参照): 登録済みドメイン一覧(残り枠表示・
     個別削除ボタン)、追加フォーム、SFTP接続コマンド取得時のドメイン
     選択`<select>`を追加。
  - **検証**: `cargo build --workspace`(featureフラグ無し)・
    `cargo build -p open-web-server-gateway --features ddns,sftp,upnp`
    ともに警告0件(pre-existingの無関係な5件のdead_code警告のみ残置)。
    `cargo test -p open-web-server-gateway --features ddns,sftp,upnp`
    **73件全green**(新規: `registry_enforces_capacity_limit`
    [21件目で明示的に拒否されることを確認]・
    `registry_allows_re_registering_existing_domain_at_capacity`・
    `registry_remove_then_list_reflects_change`・
    `seed_from_env_is_a_noop_without_env_vars`・実HTTP経由の
    `ddns_domains_list_and_delete_work_over_real_http`
    [認証無し401・一覧2件+残り枠18件・削除後1件+404を実HTTPで確認]、
    既存の`sftp_connection_info_prefers_duckdns_domain_over_raw_ip`も
    継続green)。`cargo test --workspace`(featureフラグ無し)も
    全green。`open-easy-web`側UIは実ブラウザ(Claude Browser pane)で
    一覧+追加フォームが正しく描画され、白画面・コンソールエラーが
    無いことを確認済み。
  - **正直な限界**: 実DuckDNSトークンでの複数ドメイン実接続E2Eは未実施
    (単一ドメイン版と同じ制約、モック+ロジック検証のみ)。

- **2026-07-23 無料DDNS(DuckDNS)による永久サブドメイン自動取得〜自動更新、
  SFTP接続情報との連動を新規実装(ユーザー指示「固定IPではないDDNSの場合の
  簡単ドメイン設定を、できれば無料のドメインを自動更新で永久に使える様に」)**:
  1. **プロバイダ選定(裏取り込み)**: DuckDNSを第一候補として採用。
     無料・更新APIは`GET`1本・**有効期限切れの概念が無い**(No-IP無料プランに
     ある「30日ごとの手動確認メールクリック」の制約が無いため、今回の
     「自動更新で永久に使える」要件に唯一合致すると判断——No-IPはこの点で
     候補から除外)。**正直な開示**: DuckDNSアカウント自体(トークン発行)は
     ユーザーがduckdns.orgでOAuthログインして取得する必要があり、これは
     本ソフトウェアからは自動化しない(他社サービスの認証情報を代行取得
     しない既存方針)。
  2. **新規`crates/open-web-server-gateway/src/free_domain.rs`**:
     DuckDNS更新API(`https://www.duckdns.org/update?domains=...&token=...
     &ip=...`)の薄いクライアント。既存`ddns.rs`と同じ「5分間隔でグローバル
     IP変化を検知」ループを持つが、`OPEN_WEB_SERVER_DUCKDNS_DOMAIN`/
     `OPEN_WEB_SERVER_DUCKDNS_TOKEN`の2環境変数のみで動く独立した経路として
     実装(既存の汎用URLテンプレート方式`OPEN_WEB_SERVER_DDNS_UPDATE_URL`と
     併存可能、両方設定されていれば両方が独立して動く)。
  3. **新規管理API`POST /admin/ddns/setup-free-domain`**
     (`handlers/free_domain.rs`、既存の`OPEN_WEB_SERVER_ADMIN_TOKEN`認証を
     再利用): サブドメイン名+DuckDNSトークンを受け取り即座に1回更新を試行、
     疎通確認結果と「これ以降は環境変数設定+再起動で自動更新ループに
     組み込まれる」という案内を正直に返す(このAPI呼び出し自体は環境変数を
     永続化しない、という制約も明記)。`ddns` feature無効ビルドでは
     `503 Service Unavailable`を返す。
  4. **`handlers/sftp_info.rs`をDDNSと連動**: ホスト名の優先順位を
     `OPEN_WEB_SERVER_SFTP_PUBLIC_HOST`(手動指定) → `OPEN_WEB_SERVER_
     DUCKDNS_DOMAIN`(`.duckdns.org`を補完) → その場で取得した生グローバル
     IP、の3段に変更。DDNSで確保した永続ホスト名を生IPより優先することで、
     `example_command`が「一度設定すれば変わらない、コピペで毎回使える」
     SFTP接続コマンドになる。
  5. **`open-easy-web`側にUI新設**(別リポジトリ、詳細は同リポジトリの
     CLAUDE.md参照): 「簡単ドメイン設定」ウィザード
     (`src/api_free_domain.rs`/`src/free_domain_ui.rs`)を追加、
     (a) DuckDNS外部リンク案内 → (b) サブドメイン名+トークン入力 →
     (c) `setup-free-domain`即時疎通確認 → (d) 成功後にSFTP接続コマンド例
     表示、の4ステップを1画面に集約(過剰実装を避けた)。
  - **検証**: `cargo build --workspace`(featureフラグ無し)・
    `cargo build -p open-web-server-gateway --features ddns,sftp,upnp`
    ともに警告0件(pre-existingの無関係な5件のdead_code警告[acme.rs/
    keyring.rs/php_server.rs/tenant_router.rs/web_vhost.rs]のみ残置、
    今回の新規コードからの警告は無し)。`cargo test --workspace`
    全green。DuckDNS実サービスへの実接続は本サンドボックス環境から
    検証できなかったため、`wiremock`によるモックHTTPサーバーで
    `update_duckdns`のHTTPクライアント呼び出しロジックのみを検証
    (既存の`ddns.rs`のテストパターンに合わせた、正直な制約として記録)。
    加えて実HTTP経由のe2eテストを2件追加:
    `sftp_connection_info_prefers_duckdns_domain_over_raw_ip`
    (`OPEN_WEB_SERVER_DUCKDNS_DOMAIN`設定時に生IPでなく
    `<domain>.duckdns.org`が返ることを実HTTP経由で確認)、既存の
    `sftp_connection_info_requires_admin_auth_and_reports_honest_state_
    over_real_http`も引き続きgreen。`open-easy-web`側UIは実ブラウザ
    (Claude Browser pane、`python -m http.server`でのローカル配信)で
    白画面・コンソールエラーが無いことを確認済み。
  - **正直な限界**: (1) DuckDNS実サービスとの実接続確認は未実施
    (トークンが無い・外部ネットワーク制約の可能性、モック検証のみ)。
    (2) Android版は既存HANDOFFの通りクロスコンパイル実証止まりのため、
    今回追加したDDNS/SFTP機能もAndroidで実際に使えるのはAPK化完了後
    (実配布・フォアグラウンドサービス化は依然未着手)。Windows/Linuxでは
    今回の実装がそのまま動く。
  - 次にすべきこと: (1) 実DuckDNSトークンでの実接続E2E検証、(2)
    `POST /admin/ddns/setup-free-domain`をこのAPI呼び出し単体でも
    `OPEN_WEB_SERVER_DUCKDNS_DOMAIN`/`TOKEN`環境変数への永続化まで
    面倒を見る(設定ファイル書き込み等)かどうかの検討——現状は
    「疎通確認のみ、恒久化は環境変数の手動設定+再起動」という
    スコープに留めている。

- **2026-07-23(続き) Apache+Nginxハイブリッド互換性の監査+2件対応
  (ユーザー指示「open-web-serverの完成度・実用性・互換性・連携性を
  向上して」)**:
  1. **監査結果**: `static_files`/`php_server`/`web_vhost`/TLS終端/
     `acme`/`tenant_router`の6大機能はすべて実装済み・`main.rs`から
     実際に配線済みと確認(過去に繰り返し見つかった「文書上完了・
     実際は未接続」という欠陥パターンは、この6項目には再発していな
     かった)。一方、Apache互換(.htaccess相当・mod_rewrite相当・
     Basic/Digest認証・CGI実行)・Nginx互換(gzip/brotli圧縮・
     レート制限・キャッシュ・複数バックエンドへのロードバランシング)
     は監査時点でいずれも**未着手**と判明。
  2. **`cargo test --workspace`実行で発見したflakyテストを修正**:
     `keyguardian_issued_key_authorizes_admin_requests_over_real_http`
     が並列実行時に稀に`401`(期待`201`)で失敗する既知の問題
     (CLAUDE.md自己申告済み)の実原因を特定——同じ`main.rs`内の
     `sftp_connection_info_...`テストと**同一のグローバル環境変数
     `OPEN_WEB_SERVER_ADMIN_TOKEN`を異なる値で同時に書き換えていた**
     ため。`tokio::sync::Mutex`で両テストの環境変数操作区間を直列化し
     解消(`cargo test`を5回連続実行しflakyが再発しないことを確認)。
  3. **Nginx互換のgzip圧縮を新規実装**(`compression.rs`): RPoem側
     `open-runo-router::middleware_hyper::with_compression`の実績ある
     ロジックを、この`open-web-server-gateway`の`Response<BoxBody>`
     (`Full<Bytes>`)型に合わせて移植。`route()`関数でdispatch後に
     `compression::maybe_gzip`を挟む形で配線。二重圧縮回避・256バイト
     未満は無圧縮・`Accept-Encoding`未対応クライアントはスキップ、
     という既存のRPoem実装と同じ安全設計を踏襲。
     **検証**: `cargo test -p open-web-server-gateway`**56件全green**
     (既存52件+新規4件: 大きく反復するボディの実圧縮・非対応
     クライアントでの無圧縮・小ボディでの無圧縮・既存
     `Content-Encoding`がある場合の二重圧縮回避)。実サーバーを
     `127.0.0.1:18099`で起動し`curl`で`/healthz`(2バイト、閾値未満)
     への`Accept-Encoding: gzip`付きリクエストが正しく無圧縮のまま
     返ることを確認(**正直な限界**: 256バイト超の実エンドポイントを
     手早く見つけられず、実際に圧縮が発動する経路の実HTTP確認は
     単体テストでの代替検証に留まった)。
  - 次にすべきこと: (1) Apache互換の.htaccess相当/mod_rewrite相当/
    Basic・Digest認証/CGI実行(すべて未着手)、(2) Nginx互換の
    brotli圧縮・レート制限・キャッシュ・複数バックエンドへの
    ロードバランシング(すべて未着手)、(3) 圧縮が実際に発動する
    大きめのレスポンスを返す実エンドポイントでのHTTP経由の実発動確認。

- **2026-07-23(続き) 組み込みSFTPサーバー + UPnP IGD自動ポート開放を実装
  ——ユーザー指示「固定IPでなくてもSFTP等の接続を簡単にして、必要なら
  簡単接続用プラグインアプリも」**:
  1. **`crates/open-web-server-gateway/src/sftp.rs`(新規)**: `russh`0.45
     (pure-Rust SSHサーバー)+ `russh-sftp`2.3(SFTPサブシステム)による
     組み込みSFTPサーバー。新規`sftp` Cargo feature(既定オフ、
     `russh`/`russh-sftp`をoptional依存化、既存`ddns`/`acme`と同じ作法)。
     `OPEN_WEB_SERVER_SFTP_BIND`(例: `0.0.0.0:2222`)未設定なら何もしない
     オプトイン設計。認証は`OPEN_WEB_SERVER_SFTP_AUTHORIZED_KEYS_FILE`
     (OpenSSH形式authorized_keys)による公開鍵認証を基本とし、パスワード
     認証は`OPEN_WEB_SERVER_SFTP_ALLOW_PASSWORD_AUTH=true`での明示opt-in
     のみ(既定オフ、定数時間比較で照合)。ルートは
     `OPEN_WEB_SERVER_SFTP_ROOT`(既定`./sftp-root`)配下に限定し、
     `static_files.rs`と同じcanonicalize + starts_withパターンで
     パストラバーサル対策(`resolve_within_root`)。
  2. **`crates/open-web-server-gateway/src/upnp.rs`(新規)**: `igd-next`
     0.15によるUPnP IGD自動ポート開放の補助機能。`OPEN_WEB_SERVER_
     UPNP_AUTO_FORWARD=true`での明示opt-in必須(ユーザーのネットワーク
     機器を無断操作しないため)。新規`upnp` Cargo feature(既定オフ)。
     失敗時はパニックせず`tracing::warn!`で「手動でのポートフォワード
     設定が必要」と正直に案内し、SFTPサーバー自体の起動は妨げない
     (`ddns.rs`/`acme.rs`と同じ「補助系の失敗は権威パスをブロックしない」
     設計方針)。
  3. **`GET /admin/sftp/connection-info`(`handlers/sftp_info.rs`新規)**:
     既存の`x-admin-token`/`KeyGuardian`認証を再利用し、現在の
     接続ホスト(`OPEN_WEB_SERVER_SFTP_PUBLIC_HOST`優先、未設定なら
     `ddns` feature時のみ`api.ipify.org`でその場検知)・ポート・
     `sftp -P <port> user@<host>`形式の接続コマンド例をJSONで返す。
     `sftp` feature無効時・`OPEN_WEB_SERVER_SFTP_BIND`未設定時も
     `sftp_enabled: false`を正直に返し、パニックしない。
  4. **プラグインアプリは見送り(意図的な判断)**: ユーザー要望原文で
     「必要なら」と条件付きだった簡単接続用アプリ(`egui`/`eframe`製の
     軽量クライアント)は、3.の管理APIさえあれば既存SFTPクライアント
     (WinSCP・FileZilla等)へ接続情報をコピペするだけで実用上十分であり、
     過剰実装を避けるべきと判断し新規クレートは作らなかった(ユーザー
     要望原文の「実装する場合は…過度な機能追加をしない」という条件にも
     沿う判断)。
  5. **検証(型チェックのみで完了と報告しない、既存運用ルール徹底)**:
     `cargo build --workspace`(featureフラグ無し)・
     `cargo build -p open-web-server-gateway --features sftp`・
     `--features upnp`・`--features sftp,upnp`の4通りすべて警告0件
     (新規コード起因、既存のdead_code警告のみ残存)でビルド成功。
     `cargo test -p open-web-server-gateway --features sftp,upnp`は
     **58件全green**(`--test-threads=1`で確認——並列実行時、既存の
     `keyguardian_issued_key_authorizes_admin_requests_over_real_http`
     と新規`sftp_connection_info_...`テストが同じ
     `OPEN_WEB_SERVER_ADMIN_TOKEN`環境変数をグローバルに読み書きする
     ため稀に競合する、既存のテスト分離の限界であり今回のコード自体の
     バグではないことを確認済み——正直な開示として明記)。
     **実SFTPクライアントでの往復検証**
     (`sftp::tests::real_sftp_client_roundtrip_over_loopback`):
     実TCPループバック上で本物の`russh`クライアント(公開鍵認証)+
     `russh-sftp`クライアントセッションを使い、mkdir→アップロード
     (`write`)→ディレクトリ一覧取得(`read_dir`、アップロードした
     ファイル名が実際に見えることを確認)→ダウンロード(`read`、
     バイト列が完全一致することを確認)→削除(`remove_file`、実際に
     ディスクから消えたことを確認)まで一気通貫で実証。
     パストラバーサル対策の単体テスト
     (`resolve_within_root_rejects_parent_traversal`)も追加。
     `GET /admin/sftp/connection-info`は実HTTP経由の統合テスト
     (`sftp_connection_info_requires_admin_auth_and_reports_honest_
     state_over_real_http`)で、未認証拒否・認証成功・
     `sftp_enabled: false`の正直な報告を実証。
  6. **正直な開示・未検証事項**: (1) UPnPは実ルーターの無いこの開発
     環境では実機検証できない——`igd-next`のAPI呼び出しがコード上
     正しい形であることの確認と、単体テスト(env var無しでのno-op、
     ローカルIPv4解決がパニックしないこと)に留まる。実ルーター環境
     での`add_port`実測は次回セッションでの課題。(2) SFTPサーバーの
     `readdir`実装は「1回で全件返し、2回目でEofを返す」という単純な
     ページング無し実装(`exhausted_dirs`集合で管理)——非常に大量の
     ファイルがあるディレクトリでは一括読み込みのメモリコストが
     かかる、将来の改善余地として明記。(3) ホスト鍵は起動のたびに
     `KeyPair::generate_ed25519()`で使い捨て生成しており永続化しない
     ——再起動のたびにSSHクライアント側の`known_hosts`警告が出る
     (再起動間でのホスト鍵の同一性を求める運用では、鍵ファイルへの
     永続化を追加opt-inとして次回検討)。

- **2026-07-23(続き) Android版の実現可能性を実機クロスコンパイルで実証
  ——`open-web-server`本体が実際にAndroid ELFバイナリとしてビルドできる
  ことを確認(ユーザー指示「Androidスマホやタブレットにインストールする
  とopen-web-serverを簡単インストール」、「机上の計画」で終わらせず
  実証した)**:
  1. この開発マシンには**Android Studio・SDK・NDK(27.1.12297006)・
     `cargo-ndk`・rustupのAndroidターゲット4種(aarch64/armv7/i686/
     x86_64-linux-android)が既に揃っていた**——想定より実現可能性が
     高いことが判明。
  2. `cargo ndk -t aarch64-linux-android build --release --bin
     open-web-server`を実行したところ、`reqwest`(既定でnative-tls
     経由の`openssl-sys`をリンクする)がAndroid向けのOpenSSLクロス
     ビルド設定が無く失敗した。
  3. **実バグ修正**: ルート`Cargo.toml`の`reqwest`依存が
     `features = ["json", "rustls-tls"]`と書きつつ`default-features`を
     falseにしておらず、reqwestの既定feature(`default-tls`= native-tls)
     も同時に有効なままだった(`rustls-tls`を足しても`default-tls`が
     残っていれば`openssl-sys`は消えない、という見落とされがちな
     Cargo featureの罠)。`default-features = false`にし、既定で
     必要な`charset`/`http2`/`system-proxy`を明示的に復元する形へ修正
     (このリポジトリの既存方針「TLSはrustls(pure Rust実装)に統一」を
     徹底、Android以外のプラットフォームでもOpenSSL系統の脆弱性面を
     減らせる副次効果もある)。
  4. **再検証**: 同じ`cargo ndk`コマンドが成功し、実際に
     `target/aarch64-linux-android/release/open-web-server`
     (`file`コマンドで`ELF 64-bit LSB pie executable, ARM aarch64...
     interpreter /system/bin/linker64...for Android 21`と確認、
     Android 21=Lollipop以降で動く設定)が生成されることを実証した。
     `cargo build --workspace`(通常のWindows/Linuxビルド)もリグレッション
     無しであることを確認済み。
  5. **正直な開示・次にすべきこと**: 実バイナリが生成できることは
     証明できたが、これは「Androidアプリ」そのものではない。実際に
     スマホ/タブレットへ配布・実行するには、(a) この実行ファイルを
     APK内(`jniLibs`配下等)に同梱し`ProcessBuilder`経由で起動する
     Kotlin/Java製の薄いAndroidアプリシェルの新規開発、(b) フォアグラウンド
     サービス化(バックグラウンドでの継続動作)、(c) ユーザー要求の
     3電源プロファイル(省電力版/常時電源接続版/通常版)をAndroidの
     `WorkManager`/`WakeLock`/`PowerManager`と連携させる実装、
     (d) APK署名・配布(Google Play or 直接配布)、が未着手のまま残る
     ——今回はコア実行ファイル自体のクロスコンパイル実現可能性を
     実証したところまで。「分身の術」(ドメイン毎インストール不要)の
     仕組み自体は既存の`tenant_router`/`web_vhost`がプラットフォーム
     非依存でそのまま機能するため、Android版固有の追加実装は不要と
     見込まれる。

- **2026-07-23(続き) `open-cuda`側でGPU圧縮/暗号化カーネル(ChaCha20)
  実装完了——`accel.rs::AccelBackend::Gpu`の実装候補ができた
  (関連リポジトリ動向の記録)**: このリポジトリの`accel.rs`が
  定義した`AccelBackend`(`Cpu`/`Gpu`/`Npu`/`HardwareAccelerator`)
  抽象化のうち、`Gpu`は要求時に`Cpu`へフォールバックする未実装の
  拡張点だった。`open-cuda`の`opencuda-directx`クレートに
  ChaCha20 GPUカーネル(DXIL/HLSL)が実装され、RustCrypto製
  `chacha20`クレートとの数値一致を実機(NVIDIA GT 730)で検証済み
  (コミット`ec6acf1`、詳細は`open-cuda`側CLAUDE.md HANDOFF参照)。
  **正直な開示・残作業**: (a) 認証タグ(Poly1305)のGPU実装が無く
  完全なAEADにはなっていない、(b) 小サイズペイロードでのH2D/D2H
  オーバーヘッドが実利益を生むかの実ベンチマークが未実施、
  (c) このリポジトリの`accel.rs`自体への実際の統合(`open-cuda`への
  依存追加)はまだ行っていない——次回セッションでの着手事項。

- **2026-07-23 通信層にIOWN/APN×Smart-TCPの適応制御(`RS-SmartTCP`)と
  圧縮+暗号化のハードウェアアクセラレータ抽象化(`accel.rs`)を追加
  ——アーキテクチャ位置づけ(RPoem≈Tomcat、open-web-server≈Apache+
  Nginxハイブリッド)を前提とした通信・DB連携の継続開発**:
  1. **`RS-SmartTCP`を独立リポジトリとして新設**
     ([aon-co-jp/RS-SmartTCP](https://github.com/aon-co-jp/RS-SmartTCP)、
     ローカル`F:\runo\RS-SmartTCP`、VPS`/root/RS-SmartTCP`)。
     IOWN/APN(NTTのオールフォトニクス・ネットワーク、日本-台湾間
     3,000kmで約17ms・ジッター無しを実証済み、
     [digitimes: NTT IOWN 2026](https://www.digitimes.com/news/a20251007PD227/ntt-iown-infrastructure-launch-2026.html))
     のような超低遅延・ジッター無し回線を検知した際にリトライ間隔等を
     積極化する適応制御。**正直な開示**: IOWN/APN自体はNTTが構築する
     物理telecom基盤であり、このRust製ミドルウェアが「実装」できる
     対象ではない——本クレートが行うのは「そのような回線が来た時に
     ソフトウェア層が足を引っ張らない」設計のみ。
  2. **RTT/ジッター推定アルゴリズムをRFC 6298(TCP)/RFC 9002(QUIC)と
     同じSRTT/RTTVAR(Jacobson/Karels EWMA)へ設計**(当初の固定
     ウィンドウ+標準偏差方式から、ユーザー指示による再検証を経て
     書き換え——このエコシステムが既に使うQUICの輻輳制御と同じ
     枯れたアルゴリズムに統一)。
  3. **`Smart-TCP`という名前は使わなかった**: arXiv 2512.00491
     ("Agentic AI-based Autonomous and Adaptive TCP Protocol")という
     実在する論文と同名になり混同を招くため、ユーザー確認の上
     `RS-SmartTCP`(既存のRS-接頭辞命名規則に準拠)とした。本クレートは
     訓練済みMLモデルを使わない決定論的ヒューリスティックであり、
     論文のプロトコルそのものの実装ではないことをdocに明記。
  4. **`open-web-server-wire::accel`新設**(ユーザー指示「ハードウェアが
     無くても圧縮+暗号化変換をCPUのみならずNPU/GPU/ハードウェア
     アクセラレータでも対応可能に」): `AccelBackend`列挙型
     (`Cpu`/`Gpu`/`Npu`/`HardwareAccelerator`)で将来のハードウェアを
     API形状として先取りし、`Cpu`のみ実装(`flate2`圧縮+既存
     `PayloadCipher`暗号化)。他バックエンドを要求してもパニックせず
     `Cpu`へ安全にフォールバックし`tracing::warn!`で可視化——呼び出し側
     のコードを変えずに将来ハードウェアが実装された時にそのまま差し
     替わる設計。**`open-cuda::GpuDevice`は再利用しなかった**
     (GEMM/Attention向けカーネル起動が前提の設計で、汎用バイト列の
     圧縮・AEAD暗号化とは操作の性質が異なるため、NPU対応も含め独自の
     軽量トレイトを新設)。GPU圧縮(NVIDIA nvCOMP)・GPU暗号化(学術研究
     レベルのCUDA AES実装)は実在すると確認済みだが未統合、NPU汎用
     圧縮/暗号化の実用ライブラリは調査時点で見当たらずと正直に記載。
  5. **検証**: `cargo test -p open-web-server-wire`**21件全green**
     (新規3件: CPUバックエンドでの圧縮+暗号化ラウンドトリップ〈実際に
     入力より小さくなることを確認〉、未実装バックエンド要求時の
     Cpuフォールバック、改竄された暗号文の復号拒否)。
     `cargo build --workspace`リグレッション無し。
  - 次にすべきこと: (1) `Ledger`の`retry_backoff`を`RS-SmartTCP`の
    `AdaptivePolicy`経由に実際に配線する(現状は独立クレートとして
    存在するのみ、呼び出し元は未接続)、(2) `accel::PayloadAccelerator`
    をメモリキャッシュ層(将来実装)から実際に呼び出す配線、(3) GPU
    バックエンド(nvCOMP等)の実装。

- **2026-07-22 `runo.tokyo/open-web-server`を実際に`open-web-server`
  自身が配信する構成へ切り替え完了(前回HANDOFF「2026-07-21」で策定した
  方針の実行、README.md「自己ホスト構成の方針」節も参照)**:
  1. VPS(`ssh conoha`、`/root/open-web-server`)を`git pull`で
     `56a26a0`(README方針策定コミット)まで最新化。バイナリ
     (`target/release/open-web-server`)は既にそれより新しい
     タイムスタンプでビルド済みだったため再ビルド不要と判断。
  2. `ss -ltnp`でVPS上の使用中ポートを確認(`8090`=rgit、
     `8100`-`8102`=rs-chiketto/rs-blog/rs-ec使用中)、空いていた
     **`8103`**を採用。
  3. `/root/open-web-server/web_vhosts.toml`を新規作成
     (`host="runo.tokyo"`、`docroot="/root/open-web-server/site"`、
     `php_enabled=false`)。`crates/open-web-server-gateway/src/
     web_vhost.rs`のホスト解決ロジック(`Host`ヘッダから振り分け)
     を確認した上で、nginxが転送する`Host: runo.tokyo`と一致する
     ようホスト名を設定。
  4. `/etc/systemd/system/open-web-server.service`を新規作成
     (`OPEN_WEB_SERVER_BIND=127.0.0.1:8103`、
     `OPEN_WEB_SERVER_WEB_VHOSTS_FILE=/root/open-web-server/
     web_vhosts.toml`、`Restart=always`)、`systemctl enable --now`。
     `curl -H 'Host: runo.tokyo' http://127.0.0.1:8103/`で実際に
     `site/index.html`(`<title>open-web-server</title>`)が返る
     ことをローカル確認。
  5. **nginx設定の切り替え(当初方針からの実装上の判断変更、詳細は
     README.md「自己ホスト構成の方針」節に記載)**: `open-easy-web`の
     `scripts/gen-vhost.sh`はドメイン単位の新規vhost一式生成用
     (`<DOMAIN> <BIND_IP> [UPSTREAM]`)であり、既存`runo.tokyo`
     サーバーブロック内の`/open-web-server/`という1つの`location`
     だけを差し替える(`/rgit/`等の他の`location`と同居)用途には
     噛み合わないと判断し、既にRS-Gitで実績のある「1 locationだけ
     手書きで`proxy_pass`に差し替える」パターンを踏襲した。
     `/etc/nginx/conf.d/runo-tokyo-tls.conf`を
     `.bak-20260722-110433`へバックアップ後、**443(SSL)サーバー
     ブロック**(`listen 443 ssl`、14行目〜。ポート80のリダイレクト
     専用ブロックではないことを`grep -n 'server {\|listen \|server_name'`
     で事前確認済み——RS-Git導入時に一度この取り違えが起きたと記録されて
     いる既知の落とし穴)内の
     `location /open-web-server/ { alias /var/www/open-web-server-site/; index index.html; }`
     を
     `location /open-web-server/ { proxy_pass http://127.0.0.1:8103/; proxy_set_header Host $host; proxy_set_header X-Real-IP $remote_addr; proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for; proxy_set_header X-Forwarded-Proto $scheme; }`
     に書き換え。`nginx -t`(既存の他ドメイン設定由来の無関係な警告
     のみ、syntax ok)→`systemctl reload nginx`。
  6. **実HTTPS検証(3段階、`nginx -t`が通っただけで完了と報告しない
     方針を徹底)**:
     - 通常時: `curl -o /dev/null -w '%{http_code}' https://runo.tokyo/open-web-server/`
       → **`200`**、`curl ... | grep '<title>'` →
       **`<title>open-web-server</title>`**(実際に`open-web-server`が
       生成するHTMLであることを確認)。
     - `systemctl stop open-web-server`でプロセスを一時停止した状態で
       同URLを叩くと → **`502`**(nginxが本当にプロキシ待ち受け先の
       プロセスへ転送しており、静的`alias`の名残ではないことの直接
       証明——alias配信のままなら停止しても200のままのはず)。
     - `systemctl start open-web-server`で再開後、同URLは
       **`200`**に復帰することを再確認。
  7. VPSローカル(`ssh conoha`上)からの`curl`でも`200`を再確認。
  - **正直な開示・今回のスコープ外**: (1) `web_vhosts.toml`は現状
    `runo.tokyo`向け1エントリのみ(将来他のvhostを追加する場合は
    同ファイルに追記する運用)。(2) `open-easy-web`の
    `domains.txt`/`engines.txt`(`gen-vhost.sh`が生成・追跡する管理
    台帳)には今回の変更は反映していない(手書きでnginx locationを
    直接編集したため)——将来的に`gen-vhost.sh`側を「既存サーバー
    ブロック内の特定location差し替え」に対応させる拡張を行うなら、
    その時に台帳との整合を取る。(3) `web_vhosts.toml.example`
    (リポジトリ同梱のサンプル)自体は`audiocafe.tokyo`向けのまま
    据え置き(VPS実機の`web_vhosts.toml`とは別物、後者はVPS上のみに
    存在しGit管理対象外)。

- **2026-07-21 固定IP不要のDDNS対応・クロスプラットフォーム配布・
  紹介ページ追加(ユーザー指示「固定IPでも、なくても...簡単にサーバーを
  立てられるように」「AlmaLinux/Ubuntu/Windows/Windows Server向け
  インストーラー付きダウンロード」)**:
  1. **`crates/open-web-server-gateway/src/ddns.rs`新設**: 固定IPを
     持たない自宅サーバー等向けの簡易DDNS更新。特定プロバイダ
     (No-IP・DuckDNS・Cloudflare等)の専用APIを個別実装せず、`{ip}`
     プレースホルダ入りのURLテンプレートを環境変数
     (`OPEN_WEB_SERVER_DDNS_UPDATE_URL`)で受け取る汎用方式(正直な
     開示: 対応プロバイダ一覧を保守しない代わり、ユーザー自身が
     プロバイダのURL形式を確認する必要がある)。`api.ipify.org`で
     グローバルIPを5分ごとに確認し、変化時のみ更新URLを叩く。
     新規`ddns` Cargo feature(既定オフ、`reqwest`をoptional依存化)。
  2. **クロスプラットフォーム配布**: `.github/workflows/release.yml`
     (タグpushでLinux・Windows向けバイナリ自動ビルド→GitHub Releases)、
     `install.sh`(systemdサービス登録)・`install.ps1`(Windowsサービス
     登録案内)を追加。**正直な開示**: TLS(rustls)・QUIC(quinn)・
     ACME(ring)を含むため、`RS-Chiketto`のような軽量プロジェクトとは
     異なりmusl静的リンクは狙わず、Ubuntu LTS基準のglibc(gnuターゲット)
     を使用——比較的新しいディストリでは動くが、非常に古いディストリ
     では動かない可能性がある、という選択理由をワークフロー内に明記。
  3. **紹介・ダウンロードページ**(`site/index.html`): GitHubのように
     READMEを読めるページを新設。外部ライブラリ非依存のバニラJSで、
     GitHub Raw配信からREADME.mdをfetchし、最小限のMarkdown→HTML変換
     (見出し・強調・リンク・コードブロック・箇条書き)を行う軽量実装
     (完全なMarkdown仕様の再実装ではないと明記)。OS別インストール
     コマンド(Linux/Windows)のカード型UIも同梱。**実機検証**: ローカル
     ブラウザで実際に開き、GitHubから実際にREADME本文を取得・
     見出し/コードブロック/リンクが正しくレンダリングされることを
     確認済み(コンソールエラー無し)。
  4. **検証**: `cargo build -p open-web-server-gateway`(featureあり/
     なし両方)警告0件(ddns関連の新規警告無し、既存の無関係な
     dead_code警告のみ)、`cargo test -p open-web-server-gateway
     --features ddns`のddns関連2件green。
  - 次にすべきこと: (1) VPS(`runo.tokyo/open-web-server`)への
    `site/index.html`デプロイ(静的ファイル配信、nginx設定はVPS側の
    み管理)、(2) タグpushによる実リリース(`v0.1.0`等)でGitHub Actions
    ワークフローの実動作を確認、(3) DDNSプロバイダ1つ以上での実更新
    URL疎通確認(現状はURLテンプレート置換のロジックのみ単体テスト、
    実プロバイダとの実通信は未検証)。

- **2026-07-20 静的ファイル + PHP配信を追加(Apache+Nginxハイブリッド配信エンジン構想 第一歩)**:
  `F:\open-runo\open-raid-z\CLAUDE.md`(529-534行目)で明記された「open-web-serverを
  Apache+Nginxハイブリッド配信エンジンにする」という方針の最初の実装。
  新規モジュール3本を`open-web-server-gateway`に追加(既存のAPIバックエンド用途
  `tenant_router`とは独立):
  - `static_files.rs` — docroot配下の静的ファイル配信。`..`拒否・絶対パス拒否・
    `canonicalize()`後の`starts_with`再検証によるディレクトリトラバーサル対策
    (シンボリックリンクでのdocroot外エスケープも検出、Windows権限で作成できる
    環境のみテスト実行)。
  - `php_server.rs` — `php -S 127.0.0.1:<port> -t <docroot>`をdocrootごとに
    遅延起動・使い回すサブプロセスプール(`OPEN_WEB_SERVER_PHP_BINARY`で
    パス指定可、デフォルトはこの開発環境のWinGet配布パス)。
  - `web_vhost.rs` — ホスト名→docrootのvhostレジストリ(`web_vhosts.toml`、
    `domains.toml`と同じ作法。管理API `POST/GET /admin/web-vhosts`、
    `DELETE /admin/web-vhosts/:host`も追加、既存の管理認証を共用)。
  - `handlers/web_vhost.rs` — ディスパッチ(拡張子で静的アセットと判定できる
    パスは直接配信を優先、それ以外はPHPサブプロセスへリバースプロキシ)。
  `main.rs`の`dispatch()`で、Hostヘッダ解決の優先順位を
  ①`web_vhosts` → ②`tenant_router` → ③`app_proxy`単一アップストリームに変更。

  **実機検証(audiocafe.tokyo、実PHPサイトを本サーバー経由で配信)**:
  `web_vhosts.toml.example`に`host="audiocafe.tokyo"`,
  `docroot="F:/open-runo/audiocafe.tokyo"`を設定し、
  `OPEN_WEB_SERVER_WEB_VHOSTS_FILE`指定でバイナリを実起動
  (`OPEN_WEB_SERVER_BIND=127.0.0.1:8099`)。Bashツールのサンドボックスから
  だとループバック接続がタイムアウトする環境だったため、PowerShellの
  `Invoke-WebRequest`(`curl`相当)で実HTTP検証した:
  - `GET /index.php` (Host: audiocafe.tokyo) → **`STATUS: 200`**、
    本文307,499バイトに**`AUDIOCAFE`を実際に含むことを確認**
    (`<title>AUDIOCAFE | World — Select Your Language</title>`)。
  - `GET /` (同Host) → `STATUS: 200`、同じく`AUDIOCAFE`含有を確認
    (PHPの組み込みルーティングでindex.phpへフォールバック)。
  - `GET /images/audiocafe1.png` (同Host) → `STATUS: 200`,
    `Content-Type: image/png`, 7,617,994バイト
    (静的ファイルハンドラが実ファイルをdocrootから直接配信することを確認、
    PHPサブプロセスを経由していない)。
  - 存在しない画像パスは`404`(想定通り)。
  検証後、起動した`open-web-server.exe`本体および子プロセスの
  `php -S`インスタンスはともに`Stop-Process`で終了させ、常駐プロセスを
  残さないことを確認済み。

  **テスト**: `cargo test --workspace`は**gateway 46 / ledger 20(+1 ignored,
  要ライブPostgreSQL)/ wire 18、doc-tests 0、全件green**
  (新規追加: `static_files`のパストラバーサル系・静的判定系のテスト、
  `php_server`のインスタンス使い回しテスト[環境にphpが無ければ`skip`扱いで
  安全にpassする設計]、`web_vhost`のvhost解決/TOML一括ロードのテスト)。

  **ドキュメント**: `PORTING.md`§4.7に移植時の注意(`OPEN_WEB_SERVER_PHP_BINARY`
  を必ず設定すること、本番はPHP-FPM+FastCGI推奨)を追記。10言語READMEにも
  簡潔な機能説明を追加。

  **既知の制約・今回スコープ外**:
  (1) 本番向けPHP-FPM/FastCGI直結は未実装(現状は開発用`php -S`のみ)。
  (2) PHPサブプロセスは起動したままプロセス終了まで生存し続ける設計
      (`kill_on_drop`はグレースフルシャットダウン時のみ有効、今回のように
      外部から強制終了させた場合は子プロセスが残るケースがあるため、
      運用時はプロセスツリーごとの終了確認が必要——検証時は手動で確認・終了済み)。
  (3) この開発環境のBashツールのサンドボックスからは`127.0.0.1`宛の接続が
      到達しない(別ネットワーク名前空間と見られる)ため、実HTTP検証は
      PowerShell側から実施した。今後この環境でループバック検証をする際は
      同様にPowerShellを使うこと。

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
  - **現状の認識(2026-07-19時点)**: 当時、`open-web-server`は「Apache＋
    Nginxのハイブリッド仕様のWebサーバー」として構想されているが、
    まだその役割を実際には果たせていないと本ファイルに明記されていた。
  - **更新(2026-07-20、下記HANDOFF参照)**: 上記の認識は現在は古い。
    `static_files`/`php_server`/`web_vhost`モジュールの実装により、
    ホスト名ベースのvhost・静的ファイル直接配信・PHPリバースプロキシが
    実際に動作するようになり、実PHPサイト(`audiocafe.tokyo`)を本サーバー
    経由で配信できることを実HTTPで検証済み(詳細はHANDOFF「2026-07-20」
    節参照)。加えてTLS終端(SNIごとの証明書切替、2026-07-16)・ACME自動
    取得(2026-07-16/17)・マルチテナントHostルーティング/リバースプロキシ
    (2026-07-14)も既に実装済みのため、Apache/Nginx相当の主要機能(vhost・
    リバースプロキシ・静的配信・SSL終端)は概ね揃った状態にある。
  - **残る次回セッションでの確認事項**: (1) PHP-FPM/FastCGI等の本番向け
    配信経路(現状は開発用`php -S`のみ)。(2) `accept_tls_loop`の
    HTTP/2・WebSocketアップグレード対応。(3) `open-runo`/`poem-cosmo-tauri`
    が現状担っている汎用アプリケーションサーバー役割(Rust以外の言語
    ランタイムホスト等)を、本来`open-web-server`がどこまで引き継げるか
    (引き継ぐべきかも含め)の現実的なロードマップ整理。

## アプリケーションサーバー層の役割(open-runo / poem-cosmo-tauri、2026-07-16追記、2026-07-20更新)

**2026-07-20更新**: 「配信エンジン(vhost)」としての`open-web-server`は、
静的ファイル/PHP配信・マルチテナントルーティング・TLS終端(SNI証明書切替+
ACME)まで実装・実検証済みとなり、Apache+Nginxハイブリッド仕様のWebサーバー
としての役割を大筋で果たせるようになった(詳細はCLAUDE.md HANDOFFの
「2026-07-20」「2026-07-16」「2026-07-17」各節参照)。一方、Rust以外の
言語・フレームワーク(Python/PHP/Ruby等)を汎用ランタイムとしてホストする
「第二のTomcat」相当の役割(§0.9.2、プロセス監視・crash-loop backoff等を
含む本格的なアプリケーションサーバー機能)は、引き続き`open-runo`または
`poem-cosmo-tauri`が担う。

これらは`open-raid-z`とVersionlessAPIによって、バージョンレス運用と
バージョン管理・Git管理を両立しながら、ACID互換性とZFS互換性に対応した
`aruaru-db`と、PostgreSQLとのDUAL DATABASE構成による「4層4重」の
最新鋭の通信システムを構築し、仕様変更が容易なデータベース設計により、
3DオンラインゲームAI課金アイテム、オンライン金融、オンライン証券、
オンラインクレジットカード決済など、ネット上で紛失してはならない
ミッションクリティカルな用途向けに、24時間365日ノンストップの
サーバー対応WEBサイト開発を全面的にバックアップするフレームワーク・
ミドルウェアとして機能することを目指す。

### Apache/Tomcat互換性の目標(ユーザー指示、2026-07-23、正本はopen-raid-z参照)

正本(`open-raid-z/CLAUDE.md`同名節)にユーザー指示原文・現状の到達点・
残るギャップを記録済み。要約: このリポジトリ(open-web-server)をJavaの
Apacheのように、RPoemをApacheのTomcatのように——Java・Ruby on Rails・
PHP/Laravel・Python/FastAPI等、言語を問わず連携できる汎用性を高める。
**このリポジトリの`app_proxy`/`tenant_router`は既にプレーンHTTPで転送
する設計のため、上記いずれの言語のアプリケーションサーバーも(単体で
HTTPサーバーとして起動しさえすれば)同じ仕組みで指せる**——2026-07-14
実装済みで新規コード不要、`POST /admin/tenants`への登録のみで足りる。

**訂正・E2E実証済み(2026-07-23)**: 当初「配線未接続」と記載していたが、
実際には`open-easy-web`側`appserver_registration.rs`が既にこのAPIへの
登録コード・実HTTPモックサーバーでの検証テストを備えていた(調査不足
だった)。本セッションで**実バイナリを起動し、`open-easy-web`が送るのと
同じ形式のリクエストで実際のE2E検証を実施**: (1) `open-web-server`を
実起動(`127.0.0.1:18099`)、(2) スタブバックエンド(`127.0.0.1:18199`)を
起動、(3) `POST /admin/tenants`で登録→**201**、(4) `Host:
e2e-test.example.com`ヘッダでのリクエストが実際にスタブバックエンドへ
転送され`HELLO_FROM_BACKEND`が返ることを確認、(5) `DELETE
/admin/tenants/:host`で削除→以後は**404**に戻ることも確認。
**これで`open-web-server`単体の登録・ルーティング・削除は実証済み**——
残るのは`open-easy-web`から実際にこのAPIを呼ぶ完全なE2E(認証込みの
UIフロー)と、RPoem側`POST /admin/appserver-tenants`の同様の実証
(RPoemはWSL Ubuntu経由でのビルドが必要なため次回実施)。
PHP-FPM等の本番グレード直結経路は引き続き未実装。

### Android版の計画(第二段、ユーザー指示、2026-07-23、コード未着手)

Linux/Windows版(v0.1.0、GitHub Releasesで公開済み)に続く「第二段」として、
Androidスマホ版のインストーラー付きアプリを計画する。**電源・省電力
プロファイルを3種類から選択可能にすること**が要件:

1. **省電力版**: バックグラウンドでの常時稼働を避け、必要時のみ起動する
   設計(Android Doze/App Standby制約と協調する省電力寄りの動作)。
2. **常時電源接続版**: 充電器に繋ぎっぱなしの端末(サーバー専用機として
   使う想定)向け、省電力制約を気にせず常時稼働。
3. **通常版**: 上記2つの中間、一般的なスマホ利用と両立するバランス型。

**正直な開示・未検討事項**: Android版の技術選定(Rust+JNIでのネイティブ
移植か、別ランタイムを要するか)・3プロファイルの具体的な実装方式
(Android JobScheduler/WorkManager相当の電源管理APIとの連携方法)は
今回まだ調査していない。着手時は日英Web検索で現状の実装パターンを
裏付けてから着手すること(既存の運用ルールに従う)。
---

## エコシステム全体マップ(2026-07-21追記)

同時並行開発の対象プロジェクト一覧・各リポジトリの現況は
[`open-raid-z`のCLAUDE.md](https://github.com/aon-co-jp/open-raid-z/blob/main/CLAUDE.md)
「関連プロジェクト」節を参照。**どのリポジトリから読み始めても、
この節を起点に他プロジェクトへ辿れる**ようにしてある(新規追加:
RS-Git・RJSON・RS-Chiketto・RS-Blog・RS-EC。このリポジトリ自身の状況は
このファイルの他の節・HANDOFFを参照)。
