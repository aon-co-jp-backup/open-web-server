//! `GET`/`POST /admin/power-profile` — 実行時の省メモリ/省電力プロファイル
//! 切替(2026-07-26追加、`crate::power_profile`参照)。既存の`x-admin-token`/
//! `KeyGuardian`認証パターン(`handlers::tenants::check_admin_auth`)を
//! そのまま再利用する。

use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::handlers::tenants::check_admin_auth;
use crate::power_profile::PowerProfile;
use crate::response::{json_response, read_json_body, text_response, BoxBody};
use crate::state::AppState;

#[derive(Debug, Serialize)]
struct PowerProfileResponse {
    profile: &'static str,
    label: &'static str,
}

impl From<PowerProfile> for PowerProfileResponse {
    fn from(p: PowerProfile) -> Self {
        Self {
            profile: p.pref_value(),
            label: p.label(),
        }
    }
}

/// `GET /admin/power-profile` — 現在のプロファイルを返す。
pub async fn get_power_profile(state: Arc<AppState>, req: &Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, req) {
        return resp;
    }
    json_response(StatusCode::OK, &PowerProfileResponse::from(state.power_profile.get()))
}

#[derive(Debug, Deserialize)]
struct SetPowerProfileRequest {
    profile: String,
}

/// `POST /admin/power-profile` — プロファイルを切り替える。
/// **再起動不要**: 変更は`PowerProfileRegistry`(`RwLock`)へ即座に反映され、
/// バックグラウンドのポーリングループ(`ddns`/`free_domain`、`ddns` feature
/// 配下)は次のイテレーションから新しい間隔を使う。
pub async fn set_power_profile(state: Arc<AppState>, req: Request<Incoming>) -> Response<BoxBody> {
    if let Err(resp) = check_admin_auth(&state, &req) {
        return resp;
    }

    let body: SetPowerProfileRequest = match read_json_body(req).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };

    match PowerProfile::from_pref_value(&body.profile) {
        Some(profile) => {
            state.power_profile.set(profile);
            tracing::info!(?profile, "power profile switched at runtime (no restart)");
            json_response(StatusCode::OK, &PowerProfileResponse::from(profile))
        }
        None => text_response(
            StatusCode::BAD_REQUEST,
            format!(
                "unknown profile '{}': expected one of memory_saver, power_save, normal, always_on",
                body.profile
            ),
        ),
    }
}
