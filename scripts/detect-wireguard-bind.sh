#!/usr/bin/env bash
# 「ロリポップ!固定IPアクセス」等のWireGuard型固定IPサービス向け、
# open-web-serverのバインド先IPアドレスを検出するヘルパー(2026-08-06新設)。
#
# 調査結果(2026-08-06、日英でGoogle検索、正直な開示):
# 「ロリポップ!固定IPアクセス byGMOペパボ」(https://vpn.lolipop.jp/)は
# 2025年3月にリリースされたVPN型の固定IPサービス。月額539円、
# プロバイダのISP契約とは独立に固定IPを取得できる。設定方法は
# 「WireGuardアプリへライセンスの設定ファイル(.conf)を追加するだけ」
# (公式サポートサイト https://support.vpn.lolipop.jp/ より)。
#
# **重要な設計判断**: open-web-server自体はこのサービス専用のコードを
# 一切必要としない——`OPEN_WEB_SERVER_BIND`環境変数は任意のIPアドレスへ
# bindできる設計であり、特定のネットワークインターフェースに関する
# 知識を持たない(2026-07-23のRS-LinkFusion実機検証で確認済みの既存
# 設計方針と同じ)。つまり「ロリポップ!固定IPアクセス」対応に必要なのは
# (1) WireGuardクライアントをこのマシン(またはWireGuard対応ルーター)へ
# セットアップし、(2) WireGuardインターフェースへ割り当てられたIP
# アドレスを`OPEN_WEB_SERVER_BIND`に渡すことだけ——本スクリプトは(2)の
# IPアドレス検出を自動化する。
#
# **正直な開示・未検証**: このマシンには実際のロリポップ!固定IPアクセス
# 契約が無いため、実際のWireGuard接続・実際の固定IP経由での到達性は
# 未検証。本スクリプトは「WireGuardインターフェースが存在すればその
# IPを検出する」という汎用ロジックのみで、ロリポップ固有のAPI等には
# 一切依存しない(同じ仕組みでMuuMuu VPN・他のWireGuard型固定IP
# サービスにも汎用的に使えるはずだが、これも未検証)。

set -euo pipefail

usage() {
  cat <<'EOF'
使い方: detect-wireguard-bind.sh [interface-name] [port]

WireGuard型固定IPサービス(ロリポップ!固定IPアクセス等)のセットアップ後、
割り当てられたインターフェースIPを検出し、open-web-server起動用の
OPEN_WEB_SERVER_BIND値を出力する。

例:
  ./detect-wireguard-bind.sh          # インターフェース名"wg0"、ポート80を想定
  ./detect-wireguard-bind.sh wg1 8080 # インターフェース名"wg1"、ポート8080

前提: WireGuardクライアント(wg-quick等)が既にセットアップ済みで、
指定したインターフェースが実際に起動していること(`wg show`で確認可能)。
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

IFACE="${1:-wg0}"
PORT="${2:-80}"

if command -v wg >/dev/null 2>&1; then
  if ! wg show "$IFACE" >/dev/null 2>&1; then
    echo "エラー: WireGuardインターフェース '$IFACE' が見つかりません。" >&2
    echo "'wg show' で現在アクティブなインターフェース一覧を確認してください。" >&2
    exit 1
  fi
fi

IP=""
if command -v ip >/dev/null 2>&1; then
  IP=$(ip -4 addr show "$IFACE" 2>/dev/null | grep -oP '(?<=inet\s)\d+(\.\d+){3}' | head -n1 || true)
elif command -v ifconfig >/dev/null 2>&1; then
  IP=$(ifconfig "$IFACE" 2>/dev/null | grep -oE 'inet (addr:)?[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | head -n1 || true)
fi

if [[ -z "$IP" ]]; then
  echo "エラー: インターフェース '$IFACE' のIPv4アドレスを検出できませんでした。" >&2
  echo "WireGuard接続が実際に有効化されているか確認してください。" >&2
  exit 1
fi

echo "検出したIP: $IP (interface=$IFACE)"
echo
echo "open-web-server起動例:"
echo "  OPEN_WEB_SERVER_BIND=${IP}:${PORT} ./open-web-server"
