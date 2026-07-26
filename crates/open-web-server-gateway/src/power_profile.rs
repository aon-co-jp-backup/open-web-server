//! 実行時に切り替え可能な省メモリ/省電力プロファイル(デスクトップ
//! [Windows/Linux]版、2026-07-26追加)。
//!
//! **背景**: Android版(`android/app/src/main/java/tokyo/runo/openwebserver/
//! PowerProfile.kt`)には既に4電源プロファイル
//! (`memory_saver`/`power_save`/`normal`/`always_on`)があるが、切替には
//! `MainActivity`の再起動が必要だった(`switchProfileAndRestart()`)。
//! ユーザー指示「Windows版もLINUX版もAndroidスマホ版も全てのバージョンで
//! 省メモリと省電力モードを途中からでも選択出来るようにして」を受け、
//! デスクトップ(このヘッドレスサーバーバイナリ)側にも**同じ名前・同じ
//! 意味論の4プロファイル**を導入し、プロセスを再起動せずに管理API経由で
//! 切り替えられるようにする。
//!
//! **正直なスコープ**: このバイナリは元々「省電力」「省メモリ」という
//! 概念自体を持っていなかった(`state.rs`の`accel_backend`はハードウェア
//! アクセラレータ選択であり電源管理ではない)。今回新設する実際の
//! 挙動差は、**バックグラウンドの定期ポーリング頻度**(DDNS/無料
//! サブドメインの自動更新ループ、`ddns.rs`/`free_domain.rs`、`ddns`
//! feature配下)のみ——Android版が「省電力版はポーリング間隔を延ばす」
//! という1点に施策を絞ったのと同じ考え方を踏襲する。省メモリ版
//! (`MemorySaver`)は、Android版と同じく**ポーリング間隔には手を
//! 加えない**(別軸として明確に区別する、Android側`healthPollIntervalMs()`
//! のdoc参照)。デスクトップ側に「本当にメモリ使用量を減らす」具体的な
//! キャッシュ/バッファの仕組みが今回時点で存在しないため、`MemorySaver`
//! はプロファイルの選択・命名の一貫性(Android版とのラベル統一)のみを
//! 提供し、デスクトップ固有のメモリ削減の挙動はまだ無い——過剰実装を
//! 避け、正直にそう明記する(narrow-but-real、既存運用ルール通り)。

use std::sync::RwLock;
use std::time::Duration;

/// Android版`PowerProfile.kt`と同じ4プロファイル・同じ`prefValue`文字列・
/// 同じ日本語ラベルを使う(ユーザー指示「同じ命名・意味論を揃える」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerProfile {
    MemorySaver,
    PowerSave,
    Normal,
    AlwaysOn,
}

impl PowerProfile {
    /// Android版`PowerProfile.prefValue`と完全一致させる。
    pub fn pref_value(&self) -> &'static str {
        match self {
            PowerProfile::MemorySaver => "memory_saver",
            PowerProfile::PowerSave => "power_save",
            PowerProfile::Normal => "normal",
            PowerProfile::AlwaysOn => "always_on",
        }
    }

    /// Android版`PowerProfile.label`と完全一致させる(日本語ラベル)。
    pub fn label(&self) -> &'static str {
        match self {
            PowerProfile::MemorySaver => "省メモリ",
            PowerProfile::PowerSave => "省電力",
            PowerProfile::Normal => "通常",
            PowerProfile::AlwaysOn => "常時電源接続",
        }
    }

    pub fn from_pref_value(value: &str) -> Option<Self> {
        match value {
            "memory_saver" => Some(PowerProfile::MemorySaver),
            "power_save" => Some(PowerProfile::PowerSave),
            "normal" => Some(PowerProfile::Normal),
            "always_on" => Some(PowerProfile::AlwaysOn),
            _ => None,
        }
    }
}

impl Default for PowerProfile {
    fn default() -> Self {
        PowerProfile::Normal
    }
}

/// プロセス全体で共有する現在のプロファイル(`RwLock`、`AppState`から
/// `Arc`で共有)。管理APIから書き換えられ、バックグラウンドループは
/// **毎回のイテレーションでこの値を読み直す**(起動時に一度だけ固定値を
/// キャプチャするのではない——これが「途中からでも切替可能」の実体)。
#[derive(Debug, Default)]
pub struct PowerProfileRegistry {
    current: RwLock<PowerProfile>,
}

impl PowerProfileRegistry {
    pub fn new() -> Self {
        Self {
            current: RwLock::new(PowerProfile::default()),
        }
    }

    pub fn get(&self) -> PowerProfile {
        *self.current.read().expect("power profile lock poisoned")
    }

    pub fn set(&self, profile: PowerProfile) {
        *self.current.write().expect("power profile lock poisoned") = profile;
    }
}

/// バックグラウンド定期ポーリング(DDNS/無料サブドメイン自動更新)の
/// 基準間隔に対する倍率。Android版`healthPollIntervalMs()`の考え方
/// (省電力=間隔を大きく延ばす/常時電源接続=間隔を短くし即応性を優先/
/// 省メモリ・通常=基準のまま)をそのままデスクトップの倍率へ写した。
#[cfg_attr(not(feature = "ddns"), allow(dead_code))]
fn poll_interval_multiplier(profile: PowerProfile) -> f64 {
    match profile {
        PowerProfile::PowerSave => 3.0,
        PowerProfile::AlwaysOn => 0.2,
        PowerProfile::MemorySaver | PowerProfile::Normal => 1.0,
    }
}

/// 基準のポーリング間隔(例: DDNS/free_domainループの既定5分)に、現在の
/// プロファイルの倍率を適用した実際の待機時間を返す。呼び出し側は
/// ループの毎イテレーションでこれを呼ぶこと(起動時に一度だけ計算した
/// 値をキャプチャして使い回さない——これがAndroid側の`healthPollJob`が
/// 抱えていた「起動時に固定される」制約を、デスクトップ側では最初から
/// 回避する設計)。
#[cfg_attr(not(feature = "ddns"), allow(dead_code))]
pub fn effective_poll_interval(registry: &PowerProfileRegistry, base: Duration) -> Duration {
    let multiplier = poll_interval_multiplier(registry.get());
    base.mul_f64(multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pref_values_match_android_power_profile_kt() {
        // Android側`PowerProfile.kt`の`prefValue`文字列と一致することを
        // 固定するリグレッションテスト(片方だけ変更されて食い違うことを防ぐ)。
        assert_eq!(PowerProfile::MemorySaver.pref_value(), "memory_saver");
        assert_eq!(PowerProfile::PowerSave.pref_value(), "power_save");
        assert_eq!(PowerProfile::Normal.pref_value(), "normal");
        assert_eq!(PowerProfile::AlwaysOn.pref_value(), "always_on");
    }

    #[test]
    fn from_pref_value_round_trips() {
        for p in [
            PowerProfile::MemorySaver,
            PowerProfile::PowerSave,
            PowerProfile::Normal,
            PowerProfile::AlwaysOn,
        ] {
            assert_eq!(PowerProfile::from_pref_value(p.pref_value()), Some(p));
        }
        assert_eq!(PowerProfile::from_pref_value("bogus"), None);
    }

    #[test]
    fn default_is_normal() {
        let reg = PowerProfileRegistry::new();
        assert_eq!(reg.get(), PowerProfile::Normal);
    }

    #[test]
    fn set_then_get_reflects_new_value_immediately_without_restart() {
        let reg = PowerProfileRegistry::new();
        reg.set(PowerProfile::PowerSave);
        assert_eq!(reg.get(), PowerProfile::PowerSave);
        reg.set(PowerProfile::AlwaysOn);
        assert_eq!(reg.get(), PowerProfile::AlwaysOn);
    }

    #[test]
    fn poll_interval_reflects_current_profile_live() {
        let reg = PowerProfileRegistry::new();
        let base = Duration::from_secs(300);

        assert_eq!(effective_poll_interval(&reg, base), base); // normal既定

        reg.set(PowerProfile::PowerSave);
        assert_eq!(effective_poll_interval(&reg, base), Duration::from_secs(900));

        reg.set(PowerProfile::AlwaysOn);
        assert_eq!(effective_poll_interval(&reg, base), Duration::from_secs(60));

        reg.set(PowerProfile::MemorySaver);
        assert_eq!(effective_poll_interval(&reg, base), base);
    }
}
