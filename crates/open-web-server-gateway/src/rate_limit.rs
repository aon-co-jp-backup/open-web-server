//! Nginx `limit_req`相当のリクエストレート制限(2026-08-03新設、
//! 既定無効・オプトイン)。
//!
//! Nginxの`limit_req_zone`+`limit_req`は、クライアントIPごとに
//! リーキーバケット(漏れバケツ)アルゴリズムでリクエスト頻度を制限し、
//! 超過分は`burst`まで待機キューに積むか、即座に拒否する。本実装は
//! トークンバケット(`rs-link-fusion::qos::RateLimiter`の帯域制限〈バイト/秒〉
//! と同じ発想を、リクエスト件数/秒に適用したもの——ただし本実装は
//! **待機させず即座に拒否する**(Nginxの`nodelay`相当の挙動、HTTPサーバーの
//! ワーカーを詰まらせないため)。
//!
//! **設定**: `OPEN_WEB_SERVER_RATE_LIMIT_RPS`(クライアントIPあたりの
//! 秒間許容リクエスト数)が設定されている場合のみ有効。
//! `OPEN_WEB_SERVER_RATE_LIMIT_BURST`(既定は`RPS`と同値、Nginxの
//! `burst`パラメータ相当のバースト許容量)。いずれも未設定なら機能自体が
//! 無効(既存動作を一切変えない)。
//!
//! **意図的なスコープの限定(正直な開示)**: (1) Nginxの`zone`(共有メモリ
//! セグメント、複数workerプロセス間で状態共有)相当の機構は無い——本実装は
//! 単一プロセス内の`RwLock<HashMap>`のみで、複数プロセスにまたがる共有は
//! しない(このサーバー自体が単一プロセスで複数コアを使う設計のため、
//! 現状はこれで十分)。(2) `X-Forwarded-For`等のプロキシヘッダーは信用せず、
//! 実TCP接続の送信元IPアドレス(`PeerAddr`)のみを判定基準にする——ヘッダー
//! は容易に偽装できるため、信頼できるリバースプロキシの背後で運用する
//! 場合は将来的に検討可能な拡張点として残す。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;

/// クライアントIPごとのトークンバケット状態。
struct BucketState {
    tokens: f64,
    last: Instant,
}

/// `OPEN_WEB_SERVER_RATE_LIMIT_RPS`から読み込んだ設定。
#[derive(Debug, Clone, Copy)]
pub struct RateLimitConfig {
    pub requests_per_sec: f64,
    pub burst: f64,
}

impl RateLimitConfig {
    /// 環境変数から読み込む。`OPEN_WEB_SERVER_RATE_LIMIT_RPS`が未設定・
    /// パース不能なら`None`(機能無効)。
    pub fn from_env() -> Option<Self> {
        let rps: f64 = std::env::var("OPEN_WEB_SERVER_RATE_LIMIT_RPS").ok()?.parse().ok()?;
        if rps <= 0.0 {
            return None;
        }
        let burst: f64 = std::env::var("OPEN_WEB_SERVER_RATE_LIMIT_BURST")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(rps);
        Some(Self { requests_per_sec: rps, burst: burst.max(1.0) })
    }
}

/// クライアントIPごとのリクエストレートを追跡するレジストリ。
pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: RwLock<HashMap<IpAddr, BucketState>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self { config, buckets: RwLock::new(HashMap::new()) }
    }

    /// `ip`からのリクエストを1件消費してよいか判定する。トークンが
    /// 足りなければ`false`(=拒否、Nginxの`nodelay`同様に待機させない)。
    pub async fn try_acquire(&self, ip: IpAddr) -> bool {
        let mut guard = self.buckets.write().await;
        let now = Instant::now();
        let entry = guard.entry(ip).or_insert_with(|| BucketState { tokens: self.config.burst, last: now });

        let elapsed = now.duration_since(entry.last).as_secs_f64();
        entry.last = now;
        entry.tokens = (entry.tokens + elapsed * self.config.requests_per_sec).min(self.config.burst);

        if entry.tokens >= 1.0 {
            entry.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// メモリリーク防止: 長時間非アクティブなIPのバケットを間引く
    /// (このメソッドを呼ぶかどうかは呼び出し側の任意、既定では自動実行
    /// しない——過剰実装を避け、必要になった時点でバックグラウンド
    /// タスクから呼ぶ形を想定)。
    pub async fn evict_idle(&self, idle_for: std::time::Duration) {
        let now = Instant::now();
        let mut guard = self.buckets.write().await;
        guard.retain(|_, state| now.duration_since(state.last) < idle_for);
    }

    #[cfg(test)]
    async fn bucket_count(&self) -> usize {
        self.buckets.read().await.len()
    }
}

/// `AppState`に載せる共有ハンドル。設定が無ければ`None`。
pub fn from_env() -> Option<Arc<RateLimiter>> {
    RateLimitConfig::from_env().map(|cfg| Arc::new(RateLimiter::new(cfg)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1))
    }

    #[tokio::test]
    async fn allows_requests_up_to_burst_then_rejects() {
        let limiter = RateLimiter::new(RateLimitConfig { requests_per_sec: 1.0, burst: 3.0 });
        assert!(limiter.try_acquire(ip()).await);
        assert!(limiter.try_acquire(ip()).await);
        assert!(limiter.try_acquire(ip()).await);
        // バースト容量(3)を使い切ったので、即座の4件目は拒否される。
        assert!(!limiter.try_acquire(ip()).await);
    }

    #[tokio::test]
    async fn refills_tokens_over_time() {
        let limiter = RateLimiter::new(RateLimitConfig { requests_per_sec: 100.0, burst: 1.0 });
        assert!(limiter.try_acquire(ip()).await);
        assert!(!limiter.try_acquire(ip()).await);
        // 100 req/secなら約10msで1トークン回復するはず。
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(limiter.try_acquire(ip()).await);
    }

    #[tokio::test]
    async fn different_ips_have_independent_buckets() {
        let limiter = RateLimiter::new(RateLimitConfig { requests_per_sec: 1.0, burst: 1.0 });
        let ip_a = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
        let ip_b = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2));
        assert!(limiter.try_acquire(ip_a).await);
        assert!(!limiter.try_acquire(ip_a).await);
        // 別IPは影響を受けない(独立したバケツ)。
        assert!(limiter.try_acquire(ip_b).await);
    }

    // `OPEN_WEB_SERVER_RATE_LIMIT_*`はプロセス全体のグローバル環境変数の
    // ため、他のテストと並行実行されると競合し得る(`state.rs`の
    // `ACCEL_ENV_LOCK`と同じパターン)。
    static RATE_LIMIT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn from_env_is_none_without_rps_env_var() {
        let _guard = RATE_LIMIT_ENV_LOCK.lock().unwrap();
        std::env::remove_var("OPEN_WEB_SERVER_RATE_LIMIT_RPS");
        std::env::remove_var("OPEN_WEB_SERVER_RATE_LIMIT_BURST");
        assert!(RateLimitConfig::from_env().is_none());
    }

    #[test]
    fn from_env_parses_rps_and_defaults_burst_to_rps() {
        let _guard = RATE_LIMIT_ENV_LOCK.lock().unwrap();
        std::env::set_var("OPEN_WEB_SERVER_RATE_LIMIT_RPS", "5");
        std::env::remove_var("OPEN_WEB_SERVER_RATE_LIMIT_BURST");
        let cfg = RateLimitConfig::from_env().expect("should be Some when RPS is set");
        assert_eq!(cfg.requests_per_sec, 5.0);
        assert_eq!(cfg.burst, 5.0);
        std::env::remove_var("OPEN_WEB_SERVER_RATE_LIMIT_RPS");
    }

    #[test]
    fn from_env_honors_explicit_burst() {
        let _guard = RATE_LIMIT_ENV_LOCK.lock().unwrap();
        std::env::set_var("OPEN_WEB_SERVER_RATE_LIMIT_RPS", "5");
        std::env::set_var("OPEN_WEB_SERVER_RATE_LIMIT_BURST", "20");
        let cfg = RateLimitConfig::from_env().expect("should be Some when RPS is set");
        assert_eq!(cfg.burst, 20.0);
        std::env::remove_var("OPEN_WEB_SERVER_RATE_LIMIT_RPS");
        std::env::remove_var("OPEN_WEB_SERVER_RATE_LIMIT_BURST");
    }

    #[test]
    fn from_env_is_none_for_zero_or_negative_rps() {
        let _guard = RATE_LIMIT_ENV_LOCK.lock().unwrap();
        std::env::set_var("OPEN_WEB_SERVER_RATE_LIMIT_RPS", "0");
        assert!(RateLimitConfig::from_env().is_none());
        std::env::set_var("OPEN_WEB_SERVER_RATE_LIMIT_RPS", "-5");
        assert!(RateLimitConfig::from_env().is_none());
        std::env::remove_var("OPEN_WEB_SERVER_RATE_LIMIT_RPS");
    }

    #[tokio::test]
    async fn evict_idle_removes_stale_buckets_only() {
        let limiter = RateLimiter::new(RateLimitConfig { requests_per_sec: 1.0, burst: 1.0 });
        limiter.try_acquire(ip()).await;
        assert_eq!(limiter.bucket_count().await, 1);
        limiter.evict_idle(std::time::Duration::from_secs(0)).await;
        assert_eq!(limiter.bucket_count().await, 0);
    }
}
