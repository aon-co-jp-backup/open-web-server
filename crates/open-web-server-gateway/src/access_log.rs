//! アクセスログ(構造化JSON + サイズベースのローテーション+gzip圧縮)。
//!
//! 2026-07-24追記(日英Web検索で商用Webサーバーとの機能差分を調査した
//! 結果の実装。Nginx/Apacheのベストプラクティス記事を参照——
//! 「JSON形式の構造化ログがElasticsearch/Grafana Loki等の観測基盤との
//! 親和性が高い」「サイズ/日付ベースでローテートし圧縮して保持する」
//! という2点が共通して推奨されていた)。
//!
//! 既存の`tracing`ベースのリクエストログ(`main.rs::route`内の
//! `http_request`スパン)は開発者向けの観測用途(OTLPエクスポート等)に
//! 特化しており、標準出力へ流れるのみでファイルへの永続化・ローテーション
//! は行わない。本モジュールはそれとは独立に、**運用者が監査・分析目的で
//! 参照する永続アクセスログ**を提供する(既存の`tracing`層を置き換える
//! ものではなく、並存する)。
//!
//! - 既定は無効(`OPEN_WEB_SERVER_ACCESS_LOG_PATH`未設定なら一切ファイル
//!   I/Oを行わない、既存機能への影響ゼロ)。
//! - 1行1リクエストのJSON Lines形式(`{"ts":...,"method":...,"path":...,
//!   "status":...,"elapsed_ms":...,"remote_addr":...}`)。
//! - サイズベースのローテーション(既定10MiB超で`.1.gz`へ、既存の
//!   `.1.gz`は`.2.gz`へ...と世代シフト、既定5世代保持)。ローテート時の
//!   gzip圧縮は`flate2`(既存の`compression.rs`と同じcrateを再利用、
//!   新規依存を増やさない)。
//! - ファイルI/Oは`tokio::task::spawn_blocking`へ退避する(CLAUDE.mdの
//!   「async関数内でのブロッキングI/Oは spawn_blocking へ退避」方針に
//!   従う)。書き込み失敗はリクエスト処理自体をブロックしない
//!   (`tracing::warn!`のみ、既存の監査ログ〈`audit_log::FileAuditLog`〉
//!   と同じ「権威パスを止めない」設計方針を踏襲)。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

/// アクセスログ1件分。`serde_json`でJSON Linesへシリアライズする。
#[derive(Debug, Serialize)]
struct AccessLogEntry {
    ts: String,
    method: String,
    path: String,
    status: u16,
    elapsed_ms: u64,
    remote_addr: Option<String>,
}

/// ローテーション設定。
#[derive(Debug, Clone)]
pub struct AccessLogConfig {
    pub path: PathBuf,
    pub max_bytes: u64,
    pub max_backups: u32,
}

impl AccessLogConfig {
    /// 環境変数から設定を読み込む。`OPEN_WEB_SERVER_ACCESS_LOG_PATH`が
    /// 未設定なら`None`(既定無効)。
    pub fn from_env() -> Option<Self> {
        let path = std::env::var("OPEN_WEB_SERVER_ACCESS_LOG_PATH").ok()?;
        let max_bytes = std::env::var("OPEN_WEB_SERVER_ACCESS_LOG_MAX_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10 * 1024 * 1024);
        let max_backups = std::env::var("OPEN_WEB_SERVER_ACCESS_LOG_MAX_BACKUPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
        Some(Self {
            path: PathBuf::from(path),
            max_bytes,
            max_backups,
        })
    }
}

/// アクセスロガー本体。`AppState`に1個だけ保持し、リクエストごとに
/// `log()`を呼ぶ。
pub struct AccessLogger {
    config: AccessLogConfig,
    // すべての書き込み/ローテーションをこの1本のMutexで直列化する。
    // 高頻度書き込みが単一ロックの競合になり得るが、アクセスログは
    // 監査・分析用途であり秒間数万リクエストのホットパスではない
    // (決済等の権威パスは`Ledger`/`FileAuditLog`が別途担う)ため、
    // シンプルさを優先する。
    state: Mutex<()>,
}

impl AccessLogger {
    pub fn new(config: AccessLogConfig) -> Self {
        Self { config, state: Mutex::new(()) }
    }

    /// 1リクエスト分をログへ追記する(非同期呼び出し口、実I/Oは
    /// `spawn_blocking`)。書き込み失敗はリクエスト処理をブロックしない。
    pub async fn log(
        self: &std::sync::Arc<Self>,
        method: String,
        path: String,
        status: u16,
        elapsed_ms: u64,
        remote_addr: Option<String>,
    ) {
        let this = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            let entry = AccessLogEntry {
                ts: chrono::Utc::now().to_rfc3339(),
                method,
                path,
                status,
                elapsed_ms,
                remote_addr,
            };
            this.write_entry_blocking(&entry)
        })
        .await;

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "access log write failed"),
            Err(e) => tracing::warn!(error = %e, "access log write task panicked"),
        }
    }

    fn write_entry_blocking(&self, entry: &AccessLogEntry) -> std::io::Result<()> {
        let _guard = self.state.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(parent) = self.config.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let line = serde_json::to_string(entry).unwrap_or_else(|_| "{}".to_string());

        // ローテーション判定: 既存ファイルサイズ + これから書く行が
        // 上限を超えるなら、書く前にローテートする。
        let current_len = std::fs::metadata(&self.config.path).map(|m| m.len()).unwrap_or(0);
        if current_len > 0 && current_len + line.len() as u64 + 1 > self.config.max_bytes {
            rotate(&self.config)?;
        }

        let mut file: File = OpenOptions::new().create(true).append(true).open(&self.config.path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }
}

/// 世代シフト式のローテーション: `access.log` → `access.log.1.gz` →
/// `access.log.2.gz` → ... → `max_backups`世代目は削除。
fn rotate(config: &AccessLogConfig) -> std::io::Result<()> {
    // 一番古い世代から順に押し出す(N-1 → N、そしてN+1超の`max_backups`は
    // 単純に上書き削除で「これ以上は保持しない」を実現)。
    let backup_path = |gen: u32| -> PathBuf {
        let mut p = config.path.clone();
        let mut name = p.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{gen}.gz"));
        p.set_file_name(name);
        p
    };

    if config.max_backups == 0 {
        // バックアップ保持数0 = 単に切り詰めるだけ(圧縮アーカイブは残さない)。
        std::fs::remove_file(&config.path).ok();
        return Ok(());
    }

    // 最古世代(max_backups)がすでに存在するなら削除して枠を空ける。
    let oldest = backup_path(config.max_backups);
    if oldest.exists() {
        std::fs::remove_file(&oldest)?;
    }

    // gen を大きい方から小さい方へリネームしていく(衝突を避けるため)。
    for gen in (1..config.max_backups).rev() {
        let from = backup_path(gen);
        let to = backup_path(gen + 1);
        if from.exists() {
            std::fs::rename(from, to)?;
        }
    }

    // 現行ファイルを`.1.gz`としてgzip圧縮しつつ書き出し、元ファイルは削除する。
    gzip_file(&config.path, &backup_path(1))?;
    std::fs::remove_file(&config.path)?;
    Ok(())
}

/// `src`を読み込みgzip圧縮して`dst`へ書き出す(`flate2`、`compression.rs`
/// と同じcrateを再利用)。
fn gzip_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    let data = std::fs::read(src)?;
    let out = File::create(dst)?;
    let mut encoder = flate2::write::GzEncoder::new(out, flate2::Compression::default());
    encoder.write_all(&data)?;
    encoder.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("open-web-server-access-log-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn writes_json_lines() {
        let dir = temp_dir("basic");
        let path = dir.join("access.log");
        let logger = Arc::new(AccessLogger::new(AccessLogConfig {
            path: path.clone(),
            max_bytes: 10 * 1024 * 1024,
            max_backups: 5,
        }));

        logger
            .log("GET".into(), "/healthz".into(), 200, 3, Some("127.0.0.1:1234".into()))
            .await;
        logger
            .log("POST".into(), "/charge".into(), 201, 12, None)
            .await;

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);

        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["method"], "GET");
        assert_eq!(first["path"], "/healthz");
        assert_eq!(first["status"], 200);
        assert_eq!(first["remote_addr"], "127.0.0.1:1234");

        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["method"], "POST");
        assert!(second["remote_addr"].is_null());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn rotates_when_size_exceeded_and_compresses() {
        let dir = temp_dir("rotate");
        let path = dir.join("access.log");
        // 極端に小さい上限にして、2行目でローテーションが発火するようにする。
        let logger = Arc::new(AccessLogger::new(AccessLogConfig {
            path: path.clone(),
            max_bytes: 10,
            max_backups: 3,
        }));

        logger.log("GET".into(), "/a".into(), 200, 1, None).await;
        logger.log("GET".into(), "/b".into(), 200, 1, None).await;
        logger.log("GET".into(), "/c".into(), 200, 1, None).await;

        // 現行ファイルは最後の書き込み1行のみを含む。
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents.lines().count(), 1);

        // 世代が正しくシフトされ、gzip圧縮済みのバックアップが存在する。
        let gen1 = dir.join("access.log.1.gz");
        assert!(gen1.exists(), "expected rotated backup .1.gz to exist");

        // 圧縮ファイルが実際に展開可能で、内容が壊れていないことを確認する。
        let compressed = std::fs::read(&gen1).unwrap();
        let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
        let mut decompressed = String::new();
        std::io::Read::read_to_string(&mut decoder, &mut decompressed).unwrap();
        assert!(decompressed.contains("\"path\":\"/b\""));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn generation_shift_keeps_max_backups() {
        let dir = temp_dir("shift");
        let path = dir.join("access.log");
        let logger = Arc::new(AccessLogger::new(AccessLogConfig {
            path: path.clone(),
            max_bytes: 5,
            max_backups: 2,
        }));

        // 何度もローテーションを誘発する。
        for i in 0..6 {
            logger.log("GET".into(), format!("/{i}"), 200, 1, None).await;
        }

        assert!(dir.join("access.log.1.gz").exists());
        assert!(dir.join("access.log.2.gz").exists());
        assert!(!dir.join("access.log.3.gz").exists(), "max_backups=2 を超えて保持してはいけない");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_env_disabled_by_default() {
        std::env::remove_var("OPEN_WEB_SERVER_ACCESS_LOG_PATH");
        assert!(AccessLogConfig::from_env().is_none());
    }
}
