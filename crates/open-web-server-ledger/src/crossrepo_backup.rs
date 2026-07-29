//! 横断リポジトリ自動同期バックアップ (`crossrepo_backup` feature)。
//!
//! `open-easy-web`・`open-web-server`・`open-raid-z`・`aruaru-db`
//! の4リポジトリが持つ既存のバックアップ/永続化機構をそれぞれ横断的に
//! 連携させる仕組みの第一実装。**新規の巨大インフラを作るのではなく、
//! 既存資産を再利用する**方針に基づき、以下を土台にした:
//!
//! - 退避先の抽象化は、姉妹モジュール`disaster_email_backup`と同じ
//!   `open_raid_z_core::offsite_backup::OffsiteBackupTarget`
//!   (Email/GoogleDrive/SFTPの実装が既にある)をそのまま再利用する。
//!   ローカルディスク宛先だけこのモジュール側に無かったため、
//!   [`LocalDirBackupTarget`]として新規に追加した(「RS-Redの
//!   StorageBackend抽象化パターンが参考になる」というユーザー指示に
//!   基づき検討したが、RS-Redへ直接依存はせず、既にこのリポジトリが
//!   採用済みの`OffsiteBackupTarget`にローカル実装を1つ足す方が
//!   車輪の再発明が少ないと判断した)。
//! - 定期実行ループは`open-web-server-gateway/src/ddns.rs`の
//!   `spawn_if_configured`/`run_loop`(環境変数で有効化・
//!   `tokio::time::sleep`によるバックグラウンドループ)パターンを踏襲。
//!
//! ## 単一障害点を作らない設計
//!
//! [`CrossRepoSyncCoordinator::sync_once`]は、設定された各ソース
//! (リポジトリごとのファイル一覧)を**1つずつ独立して**バックアップ先へ
//! アップロードする。1つのソースファイルの読み取り失敗・1宛先への
//! アップロード失敗が起きても、`Err`を握りつぶさず`SyncReport`に記録した
//! 上で残りのソースの処理を継続する(`open-web-server-ledger`の
//! `MultiRegionReplicator`が拡張要件(4)で採用した「補助系の失敗は権威
//! パスをブロックしない」という設計方針をここでも踏襲。ただしこの
//! モジュール自体が「補助系」であり、`Ledger::commit`の権威パスには
//! 一切関与しない——バックアップの失敗がミューテーションの成否に
//! 影響することは無い)。
//!
//! ## 正直な実装範囲
//!
//! 4リポジトリ全体の完全自動相互同期(各リポジトリのプロセスが互いを
//! 監視し合う分散システム)は大規模なため、今回実装したのは
//! **ファイルベースの一方向コピー**(設定した各リポジトリの重要ファイルを
//! 定期的にバックアップ先ディレクトリへタイムスタンプ付きでコピーする)
//! に留めた。`open-raid-z`のスナップショット機構との連携
//! (`aruaru-dist::snapshot_pairing`のようなコミット単位のトリガー連動)は
//! 今回は行わず、時間ベースのポーリングのみ(ロードマップは各リポジトリ
//! `PORTING.md`/`CLAUDE.md`参照)。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use open_raid_z_core::error::BridgeResult;
use open_raid_z_core::offsite_backup::OffsiteBackupTarget;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

/// ローカルディスク宛先。VPS/レンタルサーバーの共有フォルダも、
/// 事前にマウント/共有設定されたパスを渡せばそのまま使える
/// (SMB/NFSマウント越しでも、単なるディレクトリパスとして扱えるため)。
pub struct LocalDirBackupTarget {
    root: PathBuf,
}

impl LocalDirBackupTarget {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl OffsiteBackupTarget for LocalDirBackupTarget {
    fn target_name(&self) -> String {
        format!("local:{}", self.root.display())
    }

    fn ensure_ready(&self) -> BridgeResult<()> {
        fs::create_dir_all(&self.root).map_err(|e| {
            open_raid_z_core::error::BridgeError::OffsiteBackupFailed(format!(
                "failed to create local backup dir {}: {e}",
                self.root.display()
            ))
        })
    }

    fn upload_segment(&self, label: &str, data: &[u8]) -> BridgeResult<()> {
        self.ensure_ready()?;
        // labelは"repo/relative/path"のようなスラッシュ区切りを許容する
        // (呼び出し側がリポジトリ名でサブフォルダ分けするため)。
        let dest = self.root.join(label);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                open_raid_z_core::error::BridgeError::OffsiteBackupFailed(format!(
                    "failed to create parent dir for {}: {e}",
                    dest.display()
                ))
            })?;
        }
        fs::write(&dest, data).map_err(|e| {
            open_raid_z_core::error::BridgeError::OffsiteBackupFailed(format!(
                "failed to write {}: {e}",
                dest.display()
            ))
        })
    }

    fn list_segments(&self) -> BridgeResult<Vec<String>> {
        let mut out = Vec::new();
        if !self.root.exists() {
            return Ok(out);
        }
        for entry in walk_files(&self.root) {
            if let Ok(rel) = entry.strip_prefix(&self.root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(out)
    }

    fn download_segment(&self, label: &str) -> BridgeResult<Vec<u8>> {
        let path = self.root.join(label);
        fs::read(&path).map_err(|e| {
            open_raid_z_core::error::BridgeError::OffsiteBackupFailed(format!(
                "failed to read {}: {e}",
                path.display()
            ))
        })
    }

    fn delete_segment(&self, label: &str) -> BridgeResult<()> {
        let path = self.root.join(label);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| {
                open_raid_z_core::error::BridgeError::OffsiteBackupFailed(format!(
                    "failed to delete {}: {e}",
                    path.display()
                ))
            })?;
        }
        Ok(())
    }
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// 4リポジトリのうち1つ分の同期対象定義。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoSource {
    /// バックアップ先でのサブフォルダ名 (例: `"open-web-server"`)。
    pub repo_name: String,
    /// バックアップ対象の実ファイルパス一覧(存在しないファイルは
    /// スキップし警告ログのみ、他のファイル/リポジトリの処理は継続する)。
    pub files: Vec<PathBuf>,
}

/// 1回の同期実行の結果。ドキュメント/レポートへそのまま転記できるよう、
/// 成功/失敗をリポジトリ・ファイル単位で保持する。
#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub succeeded: Vec<String>,
    pub failed: BTreeMap<String, String>,
}

impl SyncReport {
    pub fn is_fully_successful(&self) -> bool {
        self.failed.is_empty()
    }
}

/// 横断同期コーディネータ。`target`は`Arc`で共有し、複数回の
/// `sync_once`呼び出し(定期ループ)に使い回す想定。
pub struct CrossRepoSyncCoordinator<T: OffsiteBackupTarget> {
    target: T,
    sources: Vec<RepoSource>,
}

impl<T: OffsiteBackupTarget> CrossRepoSyncCoordinator<T> {
    pub fn new(target: T, sources: Vec<RepoSource>) -> Self {
        Self { target, sources }
    }

    /// 全ソースを1回だけ同期する。**1つのファイル/リポジトリの失敗が
    /// 他のファイル/リポジトリの処理をブロックしない**(単一障害点を
    /// 作らない設計、モジュール冒頭のドキュメント参照)。
    pub fn sync_once(&self) -> SyncReport {
        let mut report = SyncReport::default();

        if let Err(e) = self.target.ensure_ready() {
            // 宛先そのものが準備できない場合でも、呼び出し元(定期ループ)を
            // panicさせず、失敗として記録するのみに留める。
            warn!(target = %self.target.target_name(), error = %e, "crossrepo_backup: target not ready");
            report
                .failed
                .insert("__target__".to_string(), format!("ensure_ready failed: {e}"));
            return report;
        }

        let run_ts = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

        for source in &self.sources {
            for file in &source.files {
                let label_key = format!("{}/{}", source.repo_name, file.display());
                match fs::read(file) {
                    Ok(bytes) => {
                        let file_name = file
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "unnamed".to_string());
                        let label = format!("{}/{}/{}", source.repo_name, run_ts, file_name);
                        match self.target.upload_segment(&label, &bytes) {
                            Ok(()) => {
                                info!(repo = %source.repo_name, file = %file.display(), label = %label, "crossrepo_backup: synced");
                                report.succeeded.push(label);
                            }
                            Err(e) => {
                                error!(repo = %source.repo_name, file = %file.display(), error = %e, "crossrepo_backup: upload failed");
                                report.failed.insert(label_key, format!("upload failed: {e}"));
                            }
                        }
                    }
                    Err(e) => {
                        // ソースファイルが存在しない/読めないのは、対象
                        // リポジトリがまだそのファイルを持っていない
                        // (例: open-easy-webの設定台帳が未生成) だけの
                        // こともあるため、警告に留めて次のファイルへ進む。
                        warn!(repo = %source.repo_name, file = %file.display(), error = %e, "crossrepo_backup: source file unreadable, skipping");
                        report.failed.insert(label_key, format!("read failed: {e}"));
                    }
                }
            }
        }

        report
    }

    /// `open-web-server-gateway/src/ddns.rs`の`run_loop`と同じ
    /// バックグラウンドループパターン(既定1時間おき)。呼び出し元が
    /// `tokio::spawn`すること。
    pub async fn run_loop(self, interval: Duration)
    where
        T: 'static,
    {
        loop {
            let report = self.sync_once();
            if report.is_fully_successful() {
                info!(succeeded = report.succeeded.len(), "crossrepo_backup: cycle completed, all sources synced");
            } else {
                warn!(
                    succeeded = report.succeeded.len(),
                    failed = report.failed.len(),
                    "crossrepo_backup: cycle completed with partial failures"
                );
            }
            tokio::time::sleep(interval).await;
        }
    }
}

/// 環境変数`CROSSREPO_BACKUP_DIR`が設定されている場合のみ、定期同期
/// ループをバックグラウンドで起動する(`ddns.rs::spawn_if_configured`と
/// 同じ「未設定ならno-op」の既定安全側パターン)。
pub fn spawn_if_configured(sources: Vec<RepoSource>) {
    let Ok(dir) = std::env::var("CROSSREPO_BACKUP_DIR") else {
        return;
    };
    let interval_secs: u64 = std::env::var("CROSSREPO_BACKUP_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);

    let target = LocalDirBackupTarget::new(dir);
    let coordinator = CrossRepoSyncCoordinator::new(target, sources);
    tokio::spawn(coordinator.run_loop(Duration::from_secs(interval_secs)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    /// 実際にディスク上へファイルが生成されることを確認する結合テスト
    /// (型チェックのみで済ませない、という検証方針に基づく)。
    #[test]
    fn sync_once_writes_real_files_for_every_source() {
        let base = std::env::temp_dir().join(format!("crossrepo_backup_test_{}", uuid::Uuid::new_v4()));
        let src_dir = base.join("src");
        let dest_dir = base.join("dest");
        fs::create_dir_all(&src_dir).unwrap();

        let ews_config = write_temp_file(&src_dir, "domains.txt", "example.com\nfoo.example.com\n");
        let ledger_audit = write_temp_file(&src_dir, "audit.log", "{\"key\":\"abc\"}\n");
        let missing = src_dir.join("does_not_exist.bin");

        let sources = vec![
            RepoSource {
                repo_name: "open-easy-web".to_string(),
                files: vec![ews_config.clone()],
            },
            RepoSource {
                repo_name: "open-web-server".to_string(),
                files: vec![ledger_audit.clone(), missing],
            },
        ];

        let target = LocalDirBackupTarget::new(&dest_dir);
        let coordinator = CrossRepoSyncCoordinator::new(target, sources);
        let report = coordinator.sync_once();

        // 1つのソースが読めなくても他は成功する(単一障害点を作らない)。
        assert_eq!(report.succeeded.len(), 2);
        assert_eq!(report.failed.len(), 1);
        assert!(report.failed.keys().any(|k| k.contains("does_not_exist.bin")));

        // 実ファイルがdest_dir配下に実在することを確認する。
        let mut found_domains = false;
        let mut found_audit = false;
        for path in walk_files(&dest_dir) {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if name == "domains.txt" {
                found_domains = true;
                let contents = fs::read_to_string(&path).unwrap();
                assert!(contents.contains("example.com"));
            }
            if name == "audit.log" {
                found_audit = true;
            }
        }
        assert!(found_domains, "domains.txt was not backed up to disk");
        assert!(found_audit, "audit.log was not backed up to disk");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn local_dir_backup_target_roundtrips_list_download_delete() {
        let base = std::env::temp_dir().join(format!("crossrepo_backup_roundtrip_{}", uuid::Uuid::new_v4()));
        let target = LocalDirBackupTarget::new(&base);
        target.ensure_ready().unwrap();
        target.upload_segment("open-raid-z/2026/snapshot.meta", b"hello").unwrap();

        let listed = target.list_segments().unwrap();
        assert!(listed.iter().any(|l| l.ends_with("snapshot.meta")));

        let data = target.download_segment("open-raid-z/2026/snapshot.meta").unwrap();
        assert_eq!(data, b"hello");

        target.delete_segment("open-raid-z/2026/snapshot.meta").unwrap();
        let listed_after = target.list_segments().unwrap();
        assert!(!listed_after.iter().any(|l| l.ends_with("snapshot.meta")));

        let _ = fs::remove_dir_all(&base);
    }
}
