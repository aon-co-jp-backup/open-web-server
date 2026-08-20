//! 自己アップデート機能(2026-08-19新設)。
//!
//! ユーザー指示「open-web-serverへの自己アップデート機能実装」への
//! 対応。`open-english`側`server/src/self_update.rs`(GitHub Releases
//! 問い合わせ→バージョン比較→ダウンロード→安全な入れ替え→ヘルス
//! チェック→ロールバック、という既に実機検証済みの設計)を土台に、
//! このリポジトリの実際の配布形態(Windows: `install.ps1`によるWindows
//! サービス登録、Linux/macOS: `install.sh`/`install-macos.sh`による
//! systemd/launchdサービス登録、いずれもInno Setupの専用アンインストーラー
//! は持たない)に合わせて設計し直した。
//!
//! ## Windows版の設計(open-englishとの違い)
//!
//! open-englishはInno Setupの`unins000.exe`を前提にしていたが、
//! このリポジトリのWindows版は`install.ps1`が生成する**Windowsサービス
//! (`OpenWebServer`)**として動くため、アンインストーラーを起動する
//! のではなく、一時バッチスクリプトが「サービス停止→新しい実行ファイル
//! で上書き→サービス再開」を行う。サービスとして動いていない(単体
//! プロセスとして直接起動された)場合は`sc query`が失敗するため、その
//! 場合はプロセスを`taskkill`してから上書き→新プロセスを直接起動する
//! フォールバック分岐を用意した。
//!
//! ## Linux/macOS版の設計
//!
//! Unix系は実行中の実行ファイルを`rename`/上書きしても既に起動済みの
//! プロセスは古いinodeを掴んだまま動作を続けられるため、ロック制約が
//! 無い——`open-english`の`apply_update_unix`と同じロジックをそのまま
//! 踏襲(tarball展開→上書きコピー→新バイナリをプローブポートで起動し
//! ヘルスチェック→成功なら実ポートで正式起動して自身終了、失敗なら
//! バックアップへロールバックして自分自身〈旧バージョン〉は継続動作)。
//!
//! ## 正直な開示・未検証事項
//!
//! - この開発機はWindowsのため、Linux/macOS側の「新バージョン検出→
//!   実際にダウンロード→自己置換→再起動」という一連の流れの実機E2E
//!   検証はできていない(コンパイル成功・単体テストの確認までに留まる)。
//! - Windows側もサービスとして実際に稼働させた状態での「サービス停止→
//!   上書き→再開」という一気通貫のE2E検証はできていない
//!   (`sc query`/`sc stop`/`sc start`呼び出しロジックのコードレビュー・
//!   単体テスト〈バージョン比較・アセット名判定〉の確認までに留まる)。
//! - 2026-08-19時点で`aon-co-jp/open-web-server`にGitHub Releaseが
//!   実在するか(v0.1.0が過去に一度作成された記録がHANDOFFにあるが、
//!   現在も残っているかは本セッションでは未確認)——リリースが無い
//!   場合は`fetch_latest_release`が404エラーを返し、`check_and_apply_
//!   update`はクラッシュせず正直にログを出して何もしない設計にして
//!   ある(既存の`ddns.rs`等と同じ「補助系の失敗は権威パスをブロック
//!   しない」方針)。

use std::path::PathBuf;

use serde::Deserialize;

const GITHUB_REPO: &str = "aon-co-jp/open-web-server";

/// 新バージョン起動後、ヘルスチェックへ猶予を与える秒数(Unix版のみ、
/// Windows側はサービス起動確認を別ロジックで行うため未使用)。
pub(crate) const HEALTH_CHECK_SECS: u64 = 12;

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

/// `"v0.5.1"`や`"0.5.1"`のようなタグ文字列を`(major, minor, patch)`へ
/// パースする。パース不可な部分は0扱い(不明時は最新とは判断しない
/// 安全側)。
pub(crate) fn parse_version(raw: &str) -> (u64, u64, u64) {
    let trimmed = raw.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.').map(|s| s.parse::<u64>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// リモート版がローカル版より新しいかどうか。
pub(crate) fn is_newer(remote: &str, local: &str) -> bool {
    parse_version(remote) > parse_version(local)
}

/// 現在の実行ファイルと同じディレクトリにある`version.json`の
/// `version`フィールドを読む。無ければ「インストール済み配布物として
/// の目印が無い」=「開発ビルド/未インストール」と判断し`None`を返す
/// (open-english側で実機テストにより発見・修正済みの実バグ——
/// `version.json`が隣に無い場合に誤って"0.0.0"扱いし無人インストールを
/// 走らせてしまう——と同じ設計方針をあらかじめ踏襲)。
fn local_version() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let path = dir.join("version.json");
    let text = std::fs::read_to_string(&path).ok()?;
    #[derive(Deserialize)]
    struct V {
        version: String,
    }
    serde_json::from_str::<V>(&text).ok().map(|v: V| v.version)
}

async fn fetch_latest_release() -> anyhow::Result<LatestRelease> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build()?;
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let res = client.get(&url).header("User-Agent", "open-web-server-self-updater").send().await?;
    if !res.status().is_success() {
        anyhow::bail!("GitHub releases API returned HTTP {}", res.status());
    }
    Ok(res.json::<LatestRelease>().await?)
}

/// `.github/workflows/release.yml`の命名(`open-web-server-windows-
/// x86_64.zip`)に一致するWindows向けzipアセットを探す。
fn windows_zip_asset(release: &LatestRelease) -> Option<&ReleaseAsset> {
    release.assets.iter().find(|a| {
        let name = a.name.to_lowercase();
        name.ends_with(".zip") && name.contains("windows")
    })
}

/// `open-web-server-linux-x86_64.tar.gz`/`open-web-server-macos-
/// aarch64.tar.gz`のようなUnix系tarballアセットを探す。実行中のOSに
/// 応じて`"linux"`/`"macos"`のいずれかを含むアセットのみ採用する。
fn unix_tarball_asset(release: &LatestRelease) -> Option<&ReleaseAsset> {
    let os_label = if cfg!(target_os = "macos") { "macos" } else { "linux" };
    release.assets.iter().find(|a| {
        let name = a.name.to_lowercase();
        name.ends_with(".tar.gz") && name.contains(os_label)
    })
}

pub(crate) async fn download_to(url: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let client = reqwest::Client::builder().build()?;
    let bytes = client.get(url).header("User-Agent", "open-web-server-self-updater").send().await?.bytes().await?;
    std::fs::write(dest, &bytes)?;
    Ok(())
}

pub(crate) fn tracing_log_if_available(msg: &str) {
    tracing::info!("{msg}");
}

pub(crate) fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn set_executable_permission(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
pub(crate) fn set_executable_permission(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

/// Windowsサービス(`OpenWebServer`、`install.ps1`が登録)として稼働中か
/// どうかを`sc query`で判定する。
#[cfg(target_os = "windows")]
fn windows_service_exists() -> bool {
    std::process::Command::new("sc")
        .args(["query", "OpenWebServer"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Windows版: 新しいzipをダウンロード済みの状態から、一時バッチ
/// スクリプトを生成しデタッチ起動した上でこのプロセス自身を終了する
/// (呼び出し元には戻らない、`std::process::exit`)。ファイルロックを
/// 解放するため、実行ファイルの上書きは必ずこのプロセス終了後に行う。
#[cfg(target_os = "windows")]
async fn apply_update_windows(zip_path: &std::path::Path) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let install_dir = exe.parent().ok_or_else(|| anyhow::anyhow!("could not resolve install directory"))?.to_path_buf();
    let exe_name = exe.file_name().and_then(|n| n.to_str()).unwrap_or("open-web-server.exe").to_string();

    // バックアップ: 上書き前にインストールディレクトリ全体を退避する
    // (open-english側の自動ロールバック機能と同じ設計)。
    let backup_dir = std::env::temp_dir().join("open-web-server-self-update-backup");
    let _ = std::fs::remove_dir_all(&backup_dir);
    copy_dir_recursive(&install_dir, &backup_dir)?;

    let extract_dir = std::env::temp_dir().join("open-web-server-self-update-extract");
    let is_service = windows_service_exists();

    let script_path = std::env::temp_dir().join("open-web-server-self-update.bat");
    let mut script = String::from("@echo off\r\nping 127.0.0.1 -n 3 > nul\r\n");
    if is_service {
        script += "sc stop OpenWebServer\r\n";
        script += "ping 127.0.0.1 -n 3 > nul\r\n";
    } else {
        script += &format!("taskkill /IM \"{exe_name}\" /F >nul 2>nul\r\n");
        script += "ping 127.0.0.1 -n 2 > nul\r\n";
    }
    script += &format!(
        "powershell -NoProfile -Command \"Expand-Archive -Path '{}' -DestinationPath '{}' -Force\"\r\n",
        zip_path.display(),
        extract_dir.display()
    );
    script += &format!("xcopy \"{}\" \"{}\" /E /Y /I >nul\r\n", extract_dir.display(), install_dir.display());
    if is_service {
        script += "sc start OpenWebServer\r\n";
        // ヘルスチェック(サービスとして起動): OPEN_WEB_SERVER_BINDの
        // 既定ポート(8080)へ到達できるかを確認し、失敗すればバック
        // アップから復元してサービスを再起動する(シンプルな1回判定、
        // open-english側の設計方針を踏襲)。
        script += &format!("ping 127.0.0.1 -n {} > nul\r\n", HEALTH_CHECK_SECS + 3);
        script += "powershell -NoProfile -Command \"try { $r = Invoke-WebRequest -UseBasicParsing -TimeoutSec 3 'http://127.0.0.1:8080/healthz'; if ($r.StatusCode -ne 200) { exit 1 } } catch { exit 1 }\"\r\n";
        script += "if %errorlevel% neq 0 (\r\n";
        script += "  sc stop OpenWebServer\r\n";
        script += "  ping 127.0.0.1 -n 2 > nul\r\n";
        script += &format!("  xcopy \"{}\" \"{}\" /E /Y /I >nul\r\n", backup_dir.display(), install_dir.display());
        script += "  sc start OpenWebServer\r\n";
        script += ")\r\n";
    } else {
        script += &format!("start \"\" \"{}\"\r\n", exe.display());
    }
    script += &format!("rd /S /Q \"{}\"\r\n", backup_dir.display());
    script += &format!("rd /S /Q \"{}\"\r\n", extract_dir.display());
    script += &format!("del \"{}\"\r\n", script_path.display());
    std::fs::write(&script_path, script)?;

    tokio::process::Command::new("cmd").args(["/C", "start", "", &script_path.to_string_lossy()]).spawn()?;

    tracing_log_if_available(
        "open-web-server self-update: launched update script (service stop/start or process restart), exiting to release file locks",
    );
    std::process::exit(0);
}

/// Linux/macOS共通版: `open-english`の`apply_update_unix`と同じロジック
/// (tarball展開→上書きコピー→プローブポートでのヘルスチェック→
/// 成功なら実ポートで正式起動して自身終了、失敗ならロールバックし
/// 継続動作)。
#[cfg(not(target_os = "windows"))]
async fn apply_update_unix(tarball_path: &std::path::Path) -> anyhow::Result<()> {
    use std::net::SocketAddr;
    use std::time::Duration;

    let exe = std::env::current_exe()?;
    let install_dir = exe.parent().ok_or_else(|| anyhow::anyhow!("could not resolve install directory"))?.to_path_buf();

    let backup_path = install_dir.join("open-web-server.bak");
    std::fs::copy(&exe, &backup_path)?;

    let extract_dir = std::env::temp_dir().join("open-web-server-self-update-extract");
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;

    let status = tokio::process::Command::new("tar")
        .args(["xzf", &tarball_path.to_string_lossy(), "-C"])
        .arg(&extract_dir)
        .status()
        .await?;
    if !status.success() {
        anyhow::bail!("tar extraction failed with status {status}");
    }

    // このリポジトリのUnix向けtarballは`install.sh`と同じフラット構成
    // (バイナリ+install.sh/install-macos.sh/uninstall-macos.shが
    // トップレベルに同居、release.ymlの`Package`ステップ参照)——
    // open-englishのような単一トップディレクトリではないため、
    // extract_dir自体をそのままinstall_dirへコピーする。
    copy_dir_recursive(&extract_dir, &install_dir)?;

    let new_exe = install_dir.join("open-web-server");
    set_executable_permission(&new_exe)?;

    let real_addr: SocketAddr = std::env::var("OPEN_WEB_SERVER_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()
        .unwrap_or_else(|_| "0.0.0.0:8080".parse().unwrap());
    let probe_addr = SocketAddr::new(real_addr.ip(), real_addr.port().wrapping_add(1));

    let mut probe_child =
        tokio::process::Command::new(&new_exe).current_dir(&install_dir).env("OPEN_WEB_SERVER_BIND", probe_addr.to_string()).spawn()?;

    if health_check_with_wait(&mut probe_child, &probe_addr).await {
        let _ = probe_child.kill().await;
        tokio::process::Command::new(&new_exe).current_dir(&install_dir).spawn()?;
        tracing_log_if_available("open-web-server self-update: new version passed health check, switching over and exiting");
        std::process::exit(0);
    }

    let _ = probe_child.kill().await;
    tracing_log_if_available(
        "open-web-server self-update: new version failed health check — rolling back to the previous binary (this process was never stopped, so no restart is needed)",
    );
    std::fs::copy(&backup_path, &new_exe)?;
    set_executable_permission(&new_exe)?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
async fn health_check_with_wait(child: &mut tokio::process::Child, addr: &std::net::SocketAddr) -> bool {
    for _ in 0..HEALTH_CHECK_SECS {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        if let Ok(Some(status)) = child.try_wait() {
            tracing_log_if_available(&format!("open-web-server self-update: new version exited immediately (status: {status})"));
            return false;
        }
    }
    health_check(addr).await
}

#[cfg(not(target_os = "windows"))]
async fn health_check(addr: &std::net::SocketAddr) -> bool {
    let url = format!("http://{addr}/healthz");
    let Ok(client) = reqwest::Client::builder().timeout(std::time::Duration::from_secs(3)).build() else {
        return false;
    };
    match client.get(&url).send().await {
        Ok(res) => res.status().is_success(),
        Err(_) => false,
    }
}

/// 起動時のメンテナンスチェックの一部として呼ぶエントリポイント。
/// GitHub Releasesを確認し、新しいバージョンがあればダウンロード→
/// 安全な入れ替えを起動する。ネットワーク断・リリース未存在(404)等は
/// 正直にログへ記録するだけで、サーバー自体の起動は妨げない。
async fn check_and_apply_update() {
    let release = match fetch_latest_release().await {
        Ok(r) => r,
        Err(e) => {
            tracing_log_if_available(&format!(
                "open-web-server self-update: could not check GitHub releases ({e}) — continuing without updating"
            ));
            return;
        }
    };

    let Some(local) = local_version() else {
        tracing_log_if_available(
            "open-web-server self-update: skipped (no version.json next to the executable — not an installed copy)",
        );
        return;
    };
    if !is_newer(&release.tag_name, &local) {
        tracing_log_if_available(&format!(
            "open-web-server self-update: up to date (local {local}, latest release {})",
            release.tag_name
        ));
        return;
    }

    let is_windows = cfg!(target_os = "windows");
    let asset = if is_windows { windows_zip_asset(&release) } else { unix_tarball_asset(&release) };
    let Some(asset) = asset else {
        let platform = if is_windows {
            "Windows zip"
        } else if cfg!(target_os = "macos") {
            "macOS tarball"
        } else {
            "Linux tarball"
        };
        tracing_log_if_available(&format!(
            "open-web-server self-update: newer release {} found but no {platform} asset attached — skipping",
            release.tag_name
        ));
        return;
    };

    tracing_log_if_available(&format!(
        "open-web-server self-update: newer release {} found (local {local}), downloading {}",
        release.tag_name, asset.name
    ));

    let dest: PathBuf = std::env::temp_dir().join(&asset.name);
    match download_to(&asset.browser_download_url, &dest).await {
        Ok(()) => {
            #[cfg(target_os = "windows")]
            let result = apply_update_windows(&dest).await;
            #[cfg(not(target_os = "windows"))]
            let result = apply_update_unix(&dest).await;
            if let Err(e) = result {
                tracing_log_if_available(&format!("open-web-server self-update: failed to launch update ({e})"));
            }
        }
        Err(e) => {
            tracing_log_if_available(&format!("open-web-server self-update: download failed ({e}) — continuing without updating"));
        }
    }
}

/// `state.auto_update.is_enabled()`が`true`の場合のみ`check_and_apply_
/// update()`を実行するゲート付きラッパー。無効時はチェック自体を
/// スキップする(GitHub APIへの無用なポーリングを避ける)。
async fn check_and_apply_update_gated(state: std::sync::Arc<crate::state::AppState>) {
    if !state.auto_update.is_enabled() {
        tracing_log_if_available("open-web-server self-update: skipped (disabled at runtime, see GET/POST /admin/auto-update)");
        return;
    }
    check_and_apply_update().await;
}

/// `run()`起動時に呼ぶ、非ブロッキングのエントリポイント。サーバー本体の
/// 起動をブロックしないよう`tokio::spawn`でバックグラウンドタスクとして
/// 実行する(`ddns`/`free_domain`等の既存の補助系タスクと同じパターン)。
///
/// 有効/無効は`state.auto_update`(`AppState`、既定値は
/// `OPEN_WEB_SERVER_AUTO_UPDATE_ENABLED`環境変数から読み込み、
/// `/admin/auto-update`で実行時に切替可能)で判定する——起動時に1回だけ
/// でなく、24時間ごとに再チェックすることで「深夜バックグラウンドで
/// 自動UPDATE」のような運用にも対応できるようにした(過去のHANDOFFに
/// 記載の構想、`open-easy-web`側の設計と揃える)。
pub fn spawn_if_configured(state: std::sync::Arc<crate::state::AppState>) {
    tokio::spawn(async move {
        // 起動直後は他の初期化(TLSリスナー等)と競合しないよう少し待つ。
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        loop {
            check_and_apply_update_gated(state.clone()).await;
            tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_strings_with_and_without_v_prefix() {
        assert_eq!(parse_version("v0.5.1"), (0, 5, 1));
        assert_eq!(parse_version("0.5.1"), (0, 5, 1));
        assert_eq!(parse_version("1.0"), (1, 0, 0));
        assert_eq!(parse_version("garbage"), (0, 0, 0));
    }

    #[test]
    fn is_newer_compares_semver_correctly() {
        assert!(is_newer("v0.6.0", "0.5.1"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.5.1", "0.5.1"));
        assert!(!is_newer("v0.5.0", "0.5.1"));
    }

    #[test]
    fn windows_zip_asset_matches_expected_naming() {
        let release = LatestRelease {
            tag_name: "v0.2.0".into(),
            assets: vec![
                ReleaseAsset { name: "open-web-server-linux-x86_64.tar.gz".into(), browser_download_url: "x".into() },
                ReleaseAsset { name: "open-web-server-windows-x86_64.zip".into(), browser_download_url: "y".into() },
            ],
        };
        let found = windows_zip_asset(&release).expect("should find windows asset");
        assert_eq!(found.name, "open-web-server-windows-x86_64.zip");
    }

    #[test]
    fn unix_tarball_asset_is_none_when_absent() {
        let release =
            LatestRelease { tag_name: "v0.2.0".into(), assets: vec![ReleaseAsset { name: "unrelated.txt".into(), browser_download_url: "x".into() }] };
        assert!(unix_tarball_asset(&release).is_none());
    }
}
