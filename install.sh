#!/bin/sh
# open-web-server インストールスクリプト(AlmaLinux/Ubuntu/Debian/Fedora/RHEL等、
# systemdを使う主要Linuxディストリ共通)。
#
# **正直な開示**: このバイナリはTLS(rustls)・QUIC(quinn)を含み、
# Ubuntu 20.04以降・Debian 11以降・AlmaLinux 8以降等、比較的新しい
# glibcを持つディストリ向けにビルドされている(muslによる完全な
# ディストリ非依存の静的リンクは対象外、詳細は
# .github/workflows/release.ymlのコメント参照)。
#
# 使い方:
#   curl -fsSL https://github.com/aon-co-jp/open-web-server/releases/latest/download/open-web-server-linux-x86_64.tar.gz | tar xz
#   sudo ./install.sh

set -eu

BIN_SRC="$(dirname "$0")/open-web-server"
INSTALL_DIR="/usr/local/bin"
SERVICE_FILE="/etc/systemd/system/open-web-server.service"

if [ "$(id -u)" -ne 0 ]; then
    echo "root権限で実行してください(例: sudo ./install.sh)" >&2
    exit 1
fi

if [ ! -f "$BIN_SRC" ]; then
    echo "open-web-server バイナリが見つかりません($BIN_SRC)。同梱のtar.gzを展開したディレクトリで実行してください。" >&2
    exit 1
fi

echo "==> バイナリを ${INSTALL_DIR}/open-web-server へ配置"
install -m 755 "$BIN_SRC" "${INSTALL_DIR}/open-web-server"

if [ ! -f "$SERVICE_FILE" ]; then
    echo "==> systemdサービスを作成(${SERVICE_FILE})"
    cat > "$SERVICE_FILE" << EOF
[Unit]
Description=open-web-server - Apache+Nginxハイブリッド仕様のRust製Webサーバー
After=network.target

[Service]
Type=simple
Environment=OPEN_WEB_SERVER_BIND=0.0.0.0:8080
# ドメイン/vhost設定・固定IP不要のDDNS更新等は環境変数で指定すること。
# 例:
#   Environment=OPEN_WEB_SERVER_DOMAINS_FILE=/etc/open-web-server/domains.toml
#   Environment=OPEN_WEB_SERVER_WEB_VHOSTS_FILE=/etc/open-web-server/web_vhosts.toml
#   Environment=OPEN_WEB_SERVER_DDNS_UPDATE_URL=https://provider/update?ip={ip}
ExecStart=${INSTALL_DIR}/open-web-server
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload
else
    echo "==> 既存のsystemdサービスが見つかったため上書きしません(${SERVICE_FILE})"
fi

echo "==> 完了。次のコマンドでドメイン設定等を行ってから起動してください:"
echo "    sudo systemctl edit open-web-server  # 環境変数を追記"
echo "    sudo systemctl enable --now open-web-server"
