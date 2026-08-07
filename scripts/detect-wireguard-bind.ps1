# 「ロリポップ!固定IPアクセス」等のWireGuard型固定IPサービス向け、
# open-web-serverのバインド先IPアドレスを検出するヘルパー(PowerShell版、2026-08-06新設)。
# 詳細な設計判断・正直な開示は detect-wireguard-bind.sh のコメント参照
# (同じロジックのWindows向け移植)。
#
# 使い方:
#   .\detect-wireguard-bind.ps1                    # インターフェース名は自動検出を試みる
#   .\detect-wireguard-bind.ps1 -InterfaceAlias "WireGuard Tunnel" -Port 8080

param(
    [string]$InterfaceAlias = "",
    [int]$Port = 80
)

$candidates = Get-NetAdapter | Where-Object { $_.InterfaceDescription -match "WireGuard" -or $_.Name -match "wg" }

if ($InterfaceAlias -eq "") {
    if ($candidates.Count -eq 0) {
        Write-Error "WireGuardインターフェースが見つかりませんでした。WireGuardクライアント(公式アプリ)でトンネルを有効化してから再実行してください。"
        exit 1
    }
    $InterfaceAlias = $candidates[0].Name
    Write-Host "WireGuardインターフェースを自動検出: $InterfaceAlias"
}

$addr = Get-NetIPAddress -InterfaceAlias $InterfaceAlias -AddressFamily IPv4 -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty IPAddress

if (-not $addr) {
    Write-Error "インターフェース '$InterfaceAlias' のIPv4アドレスを検出できませんでした。WireGuard接続が実際に有効化されているか確認してください。"
    exit 1
}

Write-Host "検出したIP: $addr (interface=$InterfaceAlias)"
Write-Host ""
Write-Host "open-web-server起動例(PowerShell):"
Write-Host "  `$env:OPEN_WEB_SERVER_BIND = `"${addr}:${Port}`""
Write-Host "  .\open-web-server.exe"
