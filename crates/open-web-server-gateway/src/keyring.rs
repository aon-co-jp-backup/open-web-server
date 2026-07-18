//! `KeyGuardian` — 自己運用型APIキーレジストリ(人間による鍵管理を不要にする)。
//!
//! ユーザー指示(2026-07-18)「第二のTomcatでREST API不要でAPIキーの
//! 自動発行・自動承認・自動廃棄で、APIキーを意識しない仕様」の
//! open-web-server側実装。設計は`RPoem`/`RCosmo`の
//! `crates/open-runo-router/src/keyring.rs`(WunderGraph Cosmo Enterprise
//! 互換の`KeyGuardian`)を踏襲するが、**RPoem側の`open_runo_db::DbBackend`
//! には依存しない**(このリポジトリは`open-web-server-ledger`という
//! 独自の永続化層を持ち、別リポジトリのcrateへ直接依存させないという
//! エコシステムの既存方針(「全く違うリポジトリのプロジェクト」)を守る
//! ため、ロジックを自己完結で再実装する)。
//!
//! - **auto-issue**: `owner`(呼び出し元の識別子)と`roles`を指定して
//!   `issue()`を呼ぶだけでキーが発行される(人間が発行フォームを
//!   操作する必要はない——将来SCIM等のプロビジョニングイベントに
//!   フックすることを想定した設計)。
//! - **auto-revoke**: `revoke_owner()`でそのownerの全キーを即座に無効化。
//! - **auto-clean**: 期限切れキーは検証時に自動削除。
//! - **auto-defend**: EWMAで学習した通常のリクエスト間隔より
//!   極端に速いリクエストが来たら自動的に一時停止(クールダウン後に
//!   自動復帰)——盗難キー・暴走スクリプトへの自衛。
//!
//! **正直な開示(v1のスコープ)**: 現状は**プロセス内メモリのみ**
//! (再起動で失われる)。`open-web-server-ledger`との統合による永続化は
//! 次段階の課題として明記する(RPoem側はPostgreSQL永続化まで実装済み、
//! こちらはまずロジック本体の移植を優先した)。

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// SHA-256のバイト列を小文字16進文字列へ変換する(`hex`クレートを
/// 新規依存に追加しないための最小実装)。
fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// プレーンテキストキーのSHA-256ハッシュ(16進)。
pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    to_hex(&hasher.finalize())
}

/// 登録済みキー1件(ハッシュをキーとして保持する)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRecord {
    pub owner: String,
    pub roles: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked: bool,
}

/// 検証結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyDecision {
    /// レジストリが空(まだ1件もキーが発行されていない) →
    /// 呼び出し側の判断に委ねる(既定では開発向けに寛容な挙動を許す)。
    RegistryEmpty,
    /// 検証成功。RBACロールを付与。
    Ok { owner: String, roles: Vec<String> },
    /// 未知・失効・期限切れのキー。
    Rejected,
    /// 異常なリクエスト頻度を検知し一時停止中。
    Suspended,
}

#[derive(Debug, Clone)]
pub struct GuardianConfig {
    /// 学習済みレート比でこの倍率を超えたら異常とみなす。
    pub anomaly_factor: f64,
    /// 異常検知が有効になるまでの観測リクエスト数(ウォームアップ)。
    pub warmup_requests: u64,
    /// 異常検知後の隔離期間。
    pub cooldown: Duration,
}

impl Default for GuardianConfig {
    fn default() -> Self {
        Self { anomaly_factor: 20.0, warmup_requests: 50, cooldown: Duration::minutes(5) }
    }
}

impl GuardianConfig {
    /// `OPEN_WEB_SERVER_KEY_ANOMALY_FACTOR` /
    /// `OPEN_WEB_SERVER_KEY_WARMUP_REQUESTS` /
    /// `OPEN_WEB_SERVER_KEY_COOLDOWN_SECS`環境変数から構成する。
    pub fn from_env() -> Self {
        let d = Self::default();
        Self {
            anomaly_factor: std::env::var("OPEN_WEB_SERVER_KEY_ANOMALY_FACTOR")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.anomaly_factor),
            warmup_requests: std::env::var("OPEN_WEB_SERVER_KEY_WARMUP_REQUESTS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d.warmup_requests),
            cooldown: Duration::seconds(
                std::env::var("OPEN_WEB_SERVER_KEY_COOLDOWN_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(d.cooldown.num_seconds()),
            ),
        }
    }
}

/// EWMAの平滑化係数。
const ALPHA: f64 = 0.3;

/// キーごとの学習済み利用状況(プロセス内メモリのみ)。
#[derive(Debug, Default, Clone)]
struct Usage {
    requests: u64,
    interval_secs: Option<f64>,
    last_seen: Option<DateTime<Utc>>,
    suspended_until: Option<DateTime<Utc>>,
}

/// 自己運用型のキーレジストリ本体。
#[derive(Debug, Default)]
pub struct KeyGuardian {
    config: GuardianConfig,
    /// ハッシュ済みキー → レコード。
    records: Mutex<HashMap<String, KeyRecord>>,
    usage: Mutex<HashMap<String, Usage>>,
}

impl KeyGuardian {
    pub fn new(config: GuardianConfig) -> Self {
        Self { config, records: Mutex::new(HashMap::new()), usage: Mutex::new(HashMap::new()) }
    }

    /// `owner`向けにキーを自動発行し、プレーンテキストを返す
    /// (この呼び出しの戻り値としてのみ存在し、以後はハッシュしか保持しない)。
    pub fn issue(&self, owner: &str, roles: Vec<String>, expires_at: Option<DateTime<Utc>>) -> String {
        let plaintext = format!("ows_{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let record = KeyRecord {
            owner: owner.to_string(),
            roles,
            created_at: Utc::now(),
            expires_at,
            revoked: false,
        };
        self.records.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(hash_key(&plaintext), record);
        plaintext
    }

    /// `owner`名義の全キーを自動失効させる。失効させた件数を返す。
    pub fn revoke_owner(&self, owner: &str) -> usize {
        let mut records = self.records.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut revoked = 0;
        for record in records.values_mut() {
            if record.owner == owner && !record.revoked {
                record.revoked = true;
                revoked += 1;
            }
        }
        revoked
    }

    /// 現在登録されている(失効していない)キーの件数。管理画面等での
    /// 可視化用。
    pub fn active_key_count(&self) -> usize {
        self.records.lock().unwrap_or_else(std::sync::PoisonError::into_inner).values().filter(|r| !r.revoked).count()
    }

    /// プレーンテキストキーを検証し、利用状況を学習・自衛判定する。
    pub fn verify(&self, key: &str, now: DateTime<Utc>) -> KeyDecision {
        let hashed = hash_key(key);

        let record = {
            let mut records = self.records.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if records.is_empty() {
                return KeyDecision::RegistryEmpty;
            }
            match records.get(&hashed).cloned() {
                Some(r) => r,
                None => return KeyDecision::Rejected,
            };
            // 期限切れなら自動クリーンして削除。
            if let Some(rec) = records.get(&hashed) {
                if let Some(expiry) = rec.expires_at {
                    if now >= expiry {
                        records.remove(&hashed);
                        return KeyDecision::Rejected;
                    }
                }
            }
            records.get(&hashed).cloned()
        };

        let Some(record) = record else {
            return KeyDecision::Rejected;
        };
        if record.revoked {
            return KeyDecision::Rejected;
        }

        // ── 自己学習による異常検知(自衛) ──────────────────────────
        let mut usage = self.usage.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let u = usage.entry(hashed).or_default();

        if let Some(until) = u.suspended_until {
            if now < until {
                return KeyDecision::Suspended;
            }
            u.suspended_until = None;
        }

        if let Some(last) = u.last_seen {
            let gap = (now - last).num_milliseconds().max(0) as f64 / 1000.0;
            let learned = u.interval_secs.unwrap_or(gap);
            if u.requests >= self.config.warmup_requests
                && learned > 0.0
                && gap > 0.0
                && learned / gap >= self.config.anomaly_factor
            {
                u.suspended_until = Some(now + self.config.cooldown);
                tracing::warn!(owner = %record.owner, "KeyGuardian: anomalous request rate — key quarantined");
                return KeyDecision::Suspended;
            }
            u.interval_secs = Some(learned * (1.0 - ALPHA) + gap * ALPHA);
        }
        u.requests = u.requests.saturating_add(1);
        u.last_seen = Some(now);

        KeyDecision::Ok { owner: record.owner, roles: record.roles }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guardian() -> KeyGuardian {
        KeyGuardian::new(GuardianConfig { anomaly_factor: 10.0, warmup_requests: 3, cooldown: Duration::seconds(60) })
    }

    #[test]
    fn empty_registry_stays_permissive() {
        let g = guardian();
        assert_eq!(g.verify("anything", Utc::now()), KeyDecision::RegistryEmpty);
    }

    #[test]
    fn issue_verify_roundtrip_with_roles() {
        let g = guardian();
        let key = g.issue("alice", vec!["developer".to_string()], None);
        assert!(key.starts_with("ows_"));

        match g.verify(&key, Utc::now()) {
            KeyDecision::Ok { owner, roles } => {
                assert_eq!(owner, "alice");
                assert_eq!(roles, vec!["developer".to_string()]);
            }
            other => panic!("expected Ok, got {other:?}"),
        }

        // レジストリが非空になった後は未知のキーを拒否する(自動的な堅牢化)。
        assert_eq!(g.verify("wrong-key", Utc::now()), KeyDecision::Rejected);
    }

    #[test]
    fn revoke_owner_kills_all_their_keys() {
        let g = guardian();
        let k1 = g.issue("bob", vec![], None);
        let k2 = g.issue("bob", vec![], None);
        let alice = g.issue("alice", vec![], None);

        assert_eq!(g.revoke_owner("bob"), 2);
        assert_eq!(g.verify(&k1, Utc::now()), KeyDecision::Rejected);
        assert_eq!(g.verify(&k2, Utc::now()), KeyDecision::Rejected);
        assert!(matches!(g.verify(&alice, Utc::now()), KeyDecision::Ok { .. }));
    }

    #[test]
    fn expired_keys_auto_clean() {
        let g = guardian();
        let key = g.issue("carol", vec![], Some(Utc::now() - Duration::seconds(1)));
        assert_eq!(g.verify(&key, Utc::now()), KeyDecision::Rejected);
        assert_eq!(g.active_key_count(), 0);
    }

    #[test]
    fn anomaly_suspends_then_auto_recovers() {
        let g = guardian();
        let key = g.issue("dave", vec![], None);
        let t0 = Utc::now();

        for i in 0..4 {
            let decision = g.verify(&key, t0 + Duration::seconds(60 * i));
            assert!(matches!(decision, KeyDecision::Ok { .. }), "warmup {i}: {decision:?}");
        }
        let after_warmup = t0 + Duration::seconds(180);

        let burst = after_warmup + Duration::milliseconds(100);
        assert_eq!(g.verify(&key, burst), KeyDecision::Suspended);
        assert_eq!(g.verify(&key, burst + Duration::seconds(10)), KeyDecision::Suspended);

        assert!(matches!(g.verify(&key, burst + Duration::seconds(61)), KeyDecision::Ok { .. }));
    }

    #[test]
    fn active_key_count_excludes_revoked_keys() {
        let g = guardian();
        g.issue("erin", vec![], None);
        g.issue("frank", vec![], None);
        assert_eq!(g.active_key_count(), 2);
        g.revoke_owner("erin");
        assert_eq!(g.active_key_count(), 1);
    }
}
