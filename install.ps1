# open-web-server インストールスクリプト(Windows / Windows Server 共通)。
#
# 使い方(管理者権限のPowerShellで):
#   Invoke-WebRequest -Uri "https://github.com/aon-co-jp/open-web-server/releases/latest/download/open-web-server-windows-x86_64.zip" -OutFile open-web-server.zip
#   Expand-Archive open-web-server.zip -DestinationPath open-web-server
#   cd open-web-server
#   .\install.ps1

#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"

$InstallDir = "C:\Program Files\open-web-server"
$ServiceName = "OpenWebServer"

Write-Host "==> インストール先: $InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$BinSrc = Join-Path $PSScriptRoot "open-web-server.exe"
if (-not (Test-Path $BinSrc)) {
    Write-Error "open-web-server.exe が見つかりません($BinSrc)。zipを展開したディレクトリで実行してください。"
    exit 1
}
Copy-Item $BinSrc -Destination $InstallDir -Force

# 自己アップデート機能(src/self_update.rs、auto-update feature、
# 2026-08-19新設)は、実行ファイルの隣にある version.json の有無で
# 「インストール済み配布物かどうか」を判定する(無い場合は開発ビルド扱いで
# 自己アップデートは常に無効)。zip配布物に version.json が同梱されていれば
# それをコピーし、無ければリリースタグ相当のバージョンをこの場で生成する。
$VersionSrc = Join-Path $PSScriptRoot "version.json"
if (Test-Path $VersionSrc) {
    Copy-Item $VersionSrc -Destination $InstallDir -Force
} else {
    $fallbackVersion = @{ version = "0.1.0" } | ConvertTo-Json
    Set-Content -Path (Join-Path $InstallDir "version.json") -Value $fallbackVersion -Encoding utf8
    Write-Host "==> version.json が同梱されていなかったため既定値(0.1.0)で生成しました"
}

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "==> 既存のWindowsサービスが見つかったため、バイナリのみ更新しました"
    Write-Host "    サービスを再起動します: Restart-Service $ServiceName"
    Restart-Service -Name $ServiceName
} else {
    # 2026-08-19変更: 従来はここで手順を表示するだけで実際のサービス登録は
    # 利用者の手動操作に委ねていた。Inno Setupインストーラー
    # (`installer/open-web-server-install.iss`)が本スクリプトを
    # `-ExecutionPolicy Bypass -File install.ps1`で非対話的に呼ぶ設計に
    # なったため、ここで実際に`New-Service`/`Start-Service`まで実行する
    # よう変更した(自己アップデート機能`self_update.rs`の
    # `windows_service_exists()`が`OpenWebServer`サービスの存在を前提と
    # しているため、このサービス登録が実際に行われないと自己アップデート
    # のWindows側ロジックが機能しない)。
    [Environment]::SetEnvironmentVariable('OPEN_WEB_SERVER_BIND', '0.0.0.0:8080', 'Machine')
    New-Service -Name $ServiceName -BinaryPathName "$InstallDir\open-web-server.exe" -DisplayName "open-web-server" -StartupType Automatic | Out-Null
    Start-Service -Name $ServiceName
    Write-Host "==> Windowsサービス($ServiceName)として登録・起動しました。"
}

Write-Host "==> 完了。"
