#!/bin/sh
# open-web-server macOS向けインストールスクリプト。
#
# **正直な開示(2026-08-06追加)**: この開発環境はWindows機であり、実際の
# macOS環境でのビルド・`launchctl load`実行・動作確認は一切行えていない。
# 検証はシェル構文検証のみに留まる。launchdのplist書式・launchctlの
# 使い方は2026年時点の日英Web検索(macOS Ventura〜Sequoia世代)で確認した
# ものだが、実機での`launchctl bootstrap`実行結果は未確認。実機で試して
# 問題が見つかった場合は、CLAUDE.mdのHANDOFFへ追記の上、本スクリプトを
# 修正すること。
#
# macOSのサービス管理はsystemdではなくlaunchdを使う。ユーザーレベルの
# LaunchAgent(~/Library/LaunchAgents/)へplistを配置する方式(既定)。
# 80/443番などroot専用ポートへbindしたい場合は、/Library/LaunchDaemons/
# へ配置しsudoで実行する必要があるが、本スクリプトは既定でユーザー
# レベル・非特権ポート(8080等)を前提とする——root権限が必要な構成は
# 次回の課題として正直に明記する。
#
# 使い方:
#   curl -fsSL https://github.com/aon-co-jp/open-web-server/releases/latest/download/open-web-server-macos-x86_64.tar.gz | tar xz
#   ./install-macos.sh

set -eu

BIN_SRC="$(dirname "$0")/open-web-server"
INSTALL_DIR="${HOME}/.local/bin"
DATA_DIR="${HOME}/Library/Application Support/open-web-server"
LAUNCH_AGENTS_DIR="${HOME}/Library/LaunchAgents"
PLIST_LABEL="jp.co.aon.open-web-server"
PLIST_FILE="${LAUNCH_AGENTS_DIR}/${PLIST_LABEL}.plist"
LOG_DIR="${HOME}/Library/Logs/open-web-server"

if [ "$(uname -s)" != "Darwin" ]; then
    echo "このスクリプトはmacOS専用です(Linuxは install.sh、Windowsは install.ps1 を使ってください)。" >&2
    exit 1
fi

if [ ! -f "$BIN_SRC" ]; then
    echo "open-web-server バイナリが見つかりません($BIN_SRC)。同梱のtar.gzを展開したディレクトリで実行してください。" >&2
    exit 1
fi

echo "==> バイナリを ${INSTALL_DIR}/open-web-server へ配置"
mkdir -p "$INSTALL_DIR"
install -m 755 "$BIN_SRC" "${INSTALL_DIR}/open-web-server"
mkdir -p "$DATA_DIR"
mkdir -p "$LOG_DIR"
mkdir -p "$LAUNCH_AGENTS_DIR"

if [ ! -f "$PLIST_FILE" ]; then
    echo "==> launchd用plistを作成(${PLIST_FILE})"
    cat > "$PLIST_FILE" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${PLIST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>${INSTALL_DIR}/open-web-server</string>
    </array>
    <key>WorkingDirectory</key>
    <string>${DATA_DIR}</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>StandardOutPath</key>
    <string>${LOG_DIR}/stdout.log</string>
    <key>StandardErrorPath</key>
    <string>${LOG_DIR}/stderr.log</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>OPEN_WEB_SERVER_BIND</key>
        <string>127.0.0.1:8080</string>
        <!-- ドメイン/vhost設定・固定IP不要のDDNS更新等は環境変数で指定すること。
             例(下記の行を編集してから `launchctl bootstrap` すること):
        <key>OPEN_WEB_SERVER_DOMAINS_FILE</key>
        <string>${DATA_DIR}/domains.toml</string>
        <key>OPEN_WEB_SERVER_WEB_VHOSTS_FILE</key>
        <string>${DATA_DIR}/web_vhosts.toml</string>
        <key>OPEN_WEB_SERVER_DDNS_UPDATE_URL</key>
        <string>https://provider/update?ip={ip}</string>
        -->
    </dict>
</dict>
</plist>
EOF
else
    echo "==> 既存のlaunchd plistが見つかったため上書きしません(${PLIST_FILE})"
fi

echo ""
echo "==> 完了。次の手順でドメイン設定等を行ってから起動してください:"
echo "    1. ${PLIST_FILE} を編集し、必要な環境変数(OPEN_WEB_SERVER_DOMAINS_FILE等)を設定する。"
echo "    2. サービスを読み込んで起動する(macOS Ventura以降推奨のサブコマンド):"
echo "         launchctl bootstrap gui/\$(id -u) ${PLIST_FILE}"
echo "       (古い \`launchctl load -w ${PLIST_FILE}\` も動作するはずだが、Appleは"
echo "        load/unloadを将来的に非推奨とする方向性を示しており、新規導入では"
echo "        bootstrap/bootoutを推奨する——2026年時点の情報、実機動作は未検証)。"
echo "    3. 状態確認: launchctl list | grep ${PLIST_LABEL}"
echo "    4. ログ確認: tail -f ${LOG_DIR}/stdout.log ${LOG_DIR}/stderr.log"
echo ""
echo "==> 80/443番等の特権ポートへbindしたい場合は、本スクリプトではなく"
echo "    /Library/LaunchDaemons/ へのシステムレベルインストール+sudoでの"
echo "    launchctl実行が必要です(今回未対応、次回の課題)。"
