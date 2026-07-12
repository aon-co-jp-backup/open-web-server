//! 独立監査/突き合わせ用トランザクションログ (拡張要件(4)-④)
//!
//! これは①PostgreSQL・②aruaru-db・(将来の)③マルチリージョン同期
//! レプリケーションのいずれとも**技術的に独立**した第4の永続化先である。
//! 目的も異なる: ①②③が「同一データの複製による可用性・耐障害性」を
//! 担うのに対し、この監査ログは「後から突き合わせて二重処理/不整合を
//! 検知するための独立した証跡」を担う——実際の金融機関で言う「主系とは
//! 別システムの冗長トランザクションログ」に相当する(CLAUDE.md 拡張要件(4)
//! ④参照)。
//!
//! ## 設計
//!
//! - **追記専用 (append-only)** のプレーンファイル。WAL (`WriteAheadLog`)
//!   とは実装が完全に別 (WALはidempotencyキーでの上書き・冪等チェックを
//!   兼ねるのに対し、この監査ログは1コミットにつき1レコードを追記するのみ、
//!   上書き・削除は一切行わない)。
//! - 各レコードは `AuditRecord` をJSON直列化した1行 + そのレコードの
//!   **SHA-256チェックサム**を同じ行に埋め込む
//!   (`aruaru-core::storage` のZFS互換チェックサム層と同じ「読み取り時に
//!   検証できるチェックサム」という考え方を、監査ログというより小さい
//!   スコープで踏襲。アルゴリズムは同じSHA-256だが実装は独立)。
//! - `scan_and_verify()` はファイル全体を読み、各行のチェックサムを再計算
//!   して破損 (bit rot/途中切断) を検出する——これが「もう1系統の独立した
//!   検証手段」としての監査ログの中核機能。
//! - `reconcile()` は、監査ログに記録された `idempotency_key` の集合と、
//!   WAL側で確定した committed キー集合を突き合わせ、
//!   (a) 監査ログにあるがWAL未確定 = 何らかの理由でコミット後の記録が
//!   欠落した疑い、(b) 監査ログに重複記録された同一キー = 二重処理の疑い、
//!   を報告する。これは実際の金融機関の「主系ログと監査ログをバッチで
//!   突き合わせ、二重処理やデータ不整合を検知する」パターンの最小実装。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use open_web_server_core::{IdempotencyKey, MutationRequest};

/// 監査ログ1レコード分。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditRecord {
    pub idempotency_key: String,
    pub account_id: String,
    pub target: String,
    /// aruaru-db等、権威パス側で確定したコミットID。まだ確定前
    /// (append時点)は `None`。
    pub db_commit_id: Option<String>,
    pub recorded_at: DateTime<Utc>,
}

impl AuditRecord {
    pub fn from_request(req: &MutationRequest) -> Self {
        Self {
            idempotency_key: req.idempotency_key.0.clone(),
            account_id: req.account_id.clone(),
            target: req.target.clone(),
            db_commit_id: None,
            recorded_at: Utc::now(),
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

/// ファイル1行の物理フォーマット: `<sha256hex> <json>\n`
fn encode_line(record: &AuditRecord) -> anyhow::Result<String> {
    let json = serde_json::to_string(record)?;
    let checksum = to_hex(&Sha256::digest(json.as_bytes()));
    Ok(format!("{checksum} {json}\n"))
}

#[derive(Debug, thiserror::Error)]
pub enum AuditLogError {
    #[error("audit log line {line_no} failed checksum verification (recorded={recorded}, computed={computed})")]
    ChecksumMismatch {
        line_no: usize,
        recorded: String,
        computed: String,
    },
    #[error("audit log line {0} is malformed (missing checksum/json separator)")]
    Malformed(usize),
}

/// 突き合わせ (reconciliation) の結果。
#[derive(Debug, Default, PartialEq)]
pub struct ReconciliationReport {
    /// 監査ログに記録されているが、WAL側で確定済みキー一覧に無いキー
    /// (コミット記録の欠落疑い)。
    pub missing_from_wal: Vec<String>,
    /// 監査ログ内で同一キーが複数回記録されているキー (二重処理疑い)。
    pub duplicate_in_audit_log: Vec<String>,
    pub total_audit_records: usize,
}

/// 独立監査ログ本体。追記専用ファイルに対する単純な mutex 直列化書き込み。
pub struct FileAuditLog {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileAuditLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    /// コミット試行の時点 (WAL先行書き込みと同じタイミング) で1レコード追記する。
    /// WAL/aruaru-db/PostgreSQLのいずれとも独立したファイルへの書き込みであり、
    /// これらの書き込みが失敗しても監査ログへの記録は独立して残る
    /// (逆にこの監査ログの書き込み失敗も、他3系統をブロックしない設計に
    /// `Ledger::commit` 側で位置づける)。
    pub fn append(&self, record: &AuditRecord) -> anyhow::Result<()> {
        let _guard = self.lock.lock().unwrap();
        let line = encode_line(record)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }

    /// ファイル全体を読み、各行のチェックサムを再計算して検証する。
    /// 破損 (サイレント破損・途中切断) を検出した最初の行でエラーを返す。
    pub fn scan_and_verify(&self) -> Result<Vec<AuditRecord>, AuditLogError> {
        let _guard = self.lock.lock().unwrap();
        let Ok(file) = std::fs::File::open(&self.path) else {
            return Ok(Vec::new());
        };
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for (idx, line) in reader.lines().enumerate() {
            let line_no = idx + 1;
            let line = line.map_err(|_| AuditLogError::Malformed(line_no))?;
            if line.is_empty() {
                continue;
            }
            let (recorded_checksum, json) = line
                .split_once(' ')
                .ok_or(AuditLogError::Malformed(line_no))?;
            let computed = to_hex(&Sha256::digest(json.as_bytes()));
            if computed != recorded_checksum {
                return Err(AuditLogError::ChecksumMismatch {
                    line_no,
                    recorded: recorded_checksum.to_string(),
                    computed,
                });
            }
            let record: AuditRecord =
                serde_json::from_str(json).map_err(|_| AuditLogError::Malformed(line_no))?;
            records.push(record);
        }
        Ok(records)
    }

    /// 監査ログ内容と、WAL側で確定済みの `IdempotencyKey` 集合を突き合わせる。
    pub fn reconcile(
        &self,
        committed_keys: &[IdempotencyKey],
    ) -> Result<ReconciliationReport, AuditLogError> {
        let records = self.scan_and_verify()?;
        let committed: std::collections::HashSet<&str> =
            committed_keys.iter().map(|k| k.0.as_str()).collect();

        let mut counts: HashMap<String, usize> = HashMap::new();
        for r in &records {
            *counts.entry(r.idempotency_key.clone()).or_insert(0) += 1;
        }

        let missing_from_wal = counts
            .keys()
            .filter(|k| !committed.contains(k.as_str()))
            .cloned()
            .collect();
        let duplicate_in_audit_log = counts
            .iter()
            .filter(|(_, &n)| n > 1)
            .map(|(k, _)| k.clone())
            .collect();

        Ok(ReconciliationReport {
            missing_from_wal,
            duplicate_in_audit_log,
            total_audit_records: records.len(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_web_server_core::IdempotencyKey;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "open-web-server-audit-log-test-{name}-{}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn sample(key: &str) -> AuditRecord {
        AuditRecord {
            idempotency_key: key.to_string(),
            account_id: "user-1".to_string(),
            target: "items".to_string(),
            db_commit_id: None,
            recorded_at: Utc::now(),
        }
    }

    #[test]
    fn append_and_scan_round_trips_records() {
        let path = tmp_path("roundtrip");
        let log = FileAuditLog::new(&path);
        log.append(&sample("key-1")).unwrap();
        log.append(&sample("key-2")).unwrap();

        let records = log.scan_and_verify().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].idempotency_key, "key-1");
        assert_eq!(records[1].idempotency_key, "key-2");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn scan_detects_checksum_corruption() {
        let path = tmp_path("corruption");
        let log = FileAuditLog::new(&path);
        log.append(&sample("key-corrupt")).unwrap();

        // ファイルを直接いじり、レコード本文だけをこっそり書き換える
        // (サイレント破損のシミュレーション)。チェックサムは元のまま。
        let contents = std::fs::read_to_string(&path).unwrap();
        let tampered = contents.replace("key-corrupt", "key-tampered");
        std::fs::write(&path, tampered).unwrap();

        let result = log.scan_and_verify();
        assert!(matches!(result, Err(AuditLogError::ChecksumMismatch { .. })));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reconcile_flags_missing_and_duplicate_keys() {
        let path = tmp_path("reconcile");
        let log = FileAuditLog::new(&path);
        log.append(&sample("committed-key")).unwrap();
        log.append(&sample("orphaned-key")).unwrap();
        log.append(&sample("dup-key")).unwrap();
        log.append(&sample("dup-key")).unwrap();

        let committed = vec![
            IdempotencyKey("committed-key".to_string()),
            IdempotencyKey("dup-key".to_string()),
        ];
        let report = log.reconcile(&committed).unwrap();

        assert_eq!(report.total_audit_records, 4);
        assert_eq!(report.missing_from_wal, vec!["orphaned-key".to_string()]);
        assert_eq!(report.duplicate_in_audit_log, vec!["dup-key".to_string()]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn scan_on_nonexistent_file_returns_empty() {
        let path = tmp_path("missing");
        let log = FileAuditLog::new(&path);
        let records = log.scan_and_verify().unwrap();
        assert!(records.is_empty());
    }
}
