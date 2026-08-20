# open-web-server アンインストールスクリプト(Windows / Windows Server 共通)。
#
# 2026-08-19新設。`installer/open-web-server-install.iss`
# (Inno Setupインストーラー)の[UninstallRun]から非対話的に呼ばれる
# ことを想定しているが、単独でも実行できる
# (`powershell -ExecutionPolicy Bypass -File uninstall.ps1`)。
# `install.ps1`が登録する`OpenWebServer`という名前のWindowsサービスを
# 停止・削除し、インストールディレクトリ内の実行ファイル・
# `version.json`を削除する(利用者が独自に置いた設定ファイル
# 〈`web_vhosts.toml`等〉は誤削除を避けるため残す)。

#Requires -RunAsAdministrator

$ErrorActionPreference = "Continue"

$InstallDir = "C:\Program Files\open-web-server"
$ServiceName = "OpenWebServer"

$existing = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "==> Windowsサービス($ServiceName)を停止・削除します"
    Stop-Service -Name $ServiceName -ErrorAction SilentlyContinue
    # `Remove-Service`はPowerShell 6+限定のため、より互換性の高い
    # `sc.exe delete`を使う(このリポジトリの他スクリプトと同じ判断)。
    sc.exe delete $ServiceName | Out-Null
} else {
    Write-Host "==> Windowsサービス($ServiceName)は見つかりませんでした(単体プロセスとしての利用と判断し、そのままファイル削除へ進みます)"
}

if (Test-Path $InstallDir) {
    $binPath = Join-Path $InstallDir "open-web-server.exe"
    $versionPath = Join-Path $InstallDir "version.json"
    if (Test-Path $binPath) {
        Remove-Item -Path $binPath -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path $versionPath) {
        Remove-Item -Path $versionPath -Force -ErrorAction SilentlyContinue
    }
    Write-Host "==> $InstallDir 内の実行ファイル・version.json を削除しました(利用者の設定ファイルは残しています)"
} else {
    Write-Host "==> インストールディレクトリ($InstallDir)が見つかりませんでした"
}

Write-Host "==> 完了。"
