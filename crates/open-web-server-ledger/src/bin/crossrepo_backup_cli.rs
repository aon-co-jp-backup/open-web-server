//! `crossrepo-backup-cli`: `open_web_server_ledger::crossrepo_backup`を
//! 4リポジトリ(open-easy-web/open-web-server/open-raid-z/aruaru-db)の
//! 実ファイルに対して1回実行するCLI。`cargo run -p open-web-server-ledger
//! --features crossrepo_backup --bin crossrepo-backup-cli`で実行できる。
//!
//! 「型チェックのみで完了と報告しない」検証方針のため、このCLIは開発者が
//! 実際にバックアップ処理を動かし、実ファイルが生成されることを目視・
//! `ls`等で確認できるようにするための最小限のエントリポイントとして
//! 用意した(HTTPハンドラへの配線は次段。まずは動く1経路を用意する)。
//!
//! 環境変数:
//! - `CROSSREPO_RUNO_ROOT`: 4リポジトリの親ディレクトリ(既定
//!   `F:\runo`。このバイナリは`open-web-server/crates/
//!   open-web-server-ledger/`配下からビルドされるため、未指定時は
//!   そこから3階層上を推測する)。
//! - `CROSSREPO_BACKUP_DIR`: バックアップ先ディレクトリ(必須。未設定
//!   ならエラー終了する——バックアップ先を誤って既定値へ書き込む事故を
//!   避けるため)。

use open_web_server_ledger::crossrepo_backup::{
    CrossRepoSyncCoordinator, LocalDirBackupTarget, RepoSource,
};
use std::path::PathBuf;

fn main() {
    tracing_subscriber::fmt::init();

    let runo_root = std::env::var("CROSSREPO_RUNO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // このバイナリのソースは
            // <runo_root>/open-web-server/crates/open-web-server-ledger/
            // 配下にあるので、コンパイル時の`CARGO_MANIFEST_DIR`から
            // 3階層上がれば<runo_root>に届く。
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../")
        });

    let Ok(dest_dir) = std::env::var("CROSSREPO_BACKUP_DIR") else {
        eprintln!("CROSSREPO_BACKUP_DIR is required (backup destination directory)");
        std::process::exit(1);
    };

    let sources = vec![
        RepoSource {
            repo_name: "open-easy-web".to_string(),
            files: vec![
                runo_root.join("open-easy-web/CLAUDE.md"),
                runo_root.join("open-easy-web/PORTING.md"),
            ],
        },
        RepoSource {
            repo_name: "open-web-server".to_string(),
            files: vec![
                runo_root.join("open-web-server/CLAUDE.md"),
                runo_root.join("open-web-server/PORTING.md"),
                runo_root.join("open-web-server/domains.toml.example"),
                runo_root.join("open-web-server/web_vhosts.toml.example"),
            ],
        },
        RepoSource {
            repo_name: "open-raid-z".to_string(),
            files: vec![
                runo_root.join("open-raid-z/CLAUDE.md"),
                runo_root.join("open-raid-z/PORTING.md"),
            ],
        },
        RepoSource {
            repo_name: "aruaru-db".to_string(),
            files: vec![
                runo_root.join("aruaru-db/CLAUDE.md"),
                runo_root.join("aruaru-db/PORTING.md"),
            ],
        },
    ];

    let target = LocalDirBackupTarget::new(&dest_dir);
    let coordinator = CrossRepoSyncCoordinator::new(target, sources);
    let report = coordinator.sync_once();

    println!(
        "crossrepo-backup-cli: {} succeeded, {} failed",
        report.succeeded.len(),
        report.failed.len()
    );
    for label in &report.succeeded {
        println!("  ok: {label}");
    }
    for (key, err) in &report.failed {
        println!("  failed: {key}: {err}");
    }

    if !report.is_fully_successful() {
        // 単一障害点にはしない方針だが、CLI呼び出し元(cron等)が
        // 部分失敗を検知できるよう、終了コードは非ゼロにする
        // (バックアップ自体は個別に成功したものだけ実施済み)。
        std::process::exit(2);
    }
}
