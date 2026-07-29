//! `GET`/`POST /admin/power-profile` — 実行時の省メモリ/省電力プロファイル
//! 切替(2026-07-26追加、同日中に単一選択→**組み合わせ可能な独立フラグ**
//! へ再設計、`crate::power_profile`参照)。既存の`x-admin-token`/
//! `KeyGuardian`認証パターン(`handlers::tenants::check_admin_auth`)を
//! そのまま再利用する。
//!
//! **ペイロード形状の変更(再設計)**: 旧`{"profile": "power_save"}`
//! (単一文字列)から `{"profiles": ["power_save", "memory_saver"]}`
//! (文字列配列、組み合わせ選択)へ変更。空配列`[]`は明示的に「通常」
//! (フラグ無し)を意味する。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::handlers::tenants::check_admin_auth;
use crate::power_profile::{effective_settings, PowerProfileFlags};
use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;

#[derive(Debug, Serialize)]
struct PowerProfileResponse {
    /// アクティブなフラグの`pref_value`一覧(順序固定、空なら通常)。
    profiles: Vec<&'static str>,
    /// アクティブなフラグの日本語ラベル一覧(空なら`["通常"]`)。
    labels: Vec<&'static str>,
    /// 現在の組み合わせから合成された実効設定値(合成ロジックの結果を
    /// API利用者からも確認できるようにする)。
    poll_interval_multiplier: f64,
    memory_cache_limit_factor: f64,
}

impl From<PowerProfileFlags> for PowerProfileResponse {
    fn from(flags: PowerProfileFlags) -> Self {
        let settings = effective_settings(flags);
        Self {
            profiles: flags.active_pref_values(),
            labels: flags.active_labels(),
            poll_interval_multiplier: settings.poll_interval_multiplier,
            memory_cache_limit_factor: settings.memory_cache_limit_factor,
        }
    }
}

/// `GET /admin/power-profile` — 現在アクティブなフラグの組み合わせを返す。
pub async fn get_power_profile(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }
    json_response(StatusCode::OK, &PowerProfileResponse::from(state.power_profile.get()))
}

#[derive(Debug, Deserialize)]
struct SetPowerProfileRequest {
    /// アクティブにしたいフラグの`pref_value`一覧。空配列`[]`は「通常」
    /// (フラグ無し)を明示的に選択することを意味する。
    profiles: Vec<String>,
}

/// `POST /admin/power-profile` — フラグの組み合わせを切り替える。
/// **再起動不要**: 変更は`PowerProfileRegistry`(`RwLock`)へ即座に反映され、
/// バックグラウンドのポーリングループ(`ddns`/`free_domain`、`ddns` feature
/// 配下)は次のイテレーションから、新しい組み合わせから合成された間隔を
/// 使う。
pub async fn set_power_profile(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, &req) {
        return resp;
    }

    let body: SetPowerProfileRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    match PowerProfileFlags::from_pref_values(&body.profiles) {
        Ok(flags) => {
            state.power_profile.set(flags);
            tracing::info!(?flags, "power profile combination switched at runtime (no restart)");
            json_response(StatusCode::OK, &PowerProfileResponse::from(flags))
        }
        Err(unknown) => text_response(
            StatusCode::BAD_REQUEST,
            format!(
                "unknown profile '{unknown}': expected any combination of memory_saver, power_save, always_on (empty array = normal)"
            ),
        ),
    }
}
