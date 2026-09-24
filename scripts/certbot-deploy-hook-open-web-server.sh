#!/usr/bin/env bash
# certbot deploy hook(2026-09-24 追加): 証明書が新しく取得・更新されたら、
# open-web-server が読む TLS_CERT_DIR へコピーし、稼働中のプロセスへ再起動なしで登録する。
#
# 背景: open-web-server は起動時に TLS_CERT_DIR(<名前>.pem / <名前>.key)を読むが、
# certbot の更新結果(/etc/letsencrypt/live/...)をそこへ反映する仕組みが無かった。
# certbot は RENEWED_LINEAGE(live/<名前>/)と RENEWED_DOMAINS(証明書の全ドメイン名)を渡す。
set -uo pipefail
log() { logger -t certbot-open-web-server "$*"; echo "$*"; }

envs=$(systemctl show -p Environment --value open-web-server.service | tr ' ' '\n')
files=$(systemctl show -p EnvironmentFiles --value open-web-server.service | grep -oE '/[^ (]+')
get() { local v; v=$(printf '%s\n' "$envs" | sed -n "s/^$1=//p"); [ -z "$v" ] && for f in $files; do v=$(sed -n "s/^$1=//p" "$f" 2>/dev/null | tr -d '"\r'); [ -n "$v" ] && break; done; printf '%s' "$v"; }
certdir=$(get OPEN_WEB_SERVER_TLS_CERT_DIR)
tok=$(get OPEN_WEB_SERVER_ADMIN_TOKEN)
[ -n "$certdir" ] && [ -d "$certdir" ] || { log "TLS_CERT_DIR が見つかりません"; exit 1; }
[ -n "${RENEWED_LINEAGE:-}" ] && [ -n "${RENEWED_DOMAINS:-}" ] || { log "certbot から呼ばれていません"; exit 1; }

rc=0
for name in $RENEWED_DOMAINS; do
  install -m 644 "$RENEWED_LINEAGE/fullchain.pem" "$certdir/$name.pem"
  install -m 600 "$RENEWED_LINEAGE/privkey.pem" "$certdir/$name.key"
  if [ -n "$tok" ]; then
    body=$(jq -n --rawfile c "$RENEWED_LINEAGE/fullchain.pem" --rawfile k "$RENEWED_LINEAGE/privkey.pem" '{cert_pem:$c, key_pem:$k}')
    res=$(curl -s -w ' [HTTP %{http_code}]' -X POST "http://127.0.0.1/admin/tenants/$name/tls" \
      -H "x-admin-token: $tok" -H 'content-type: application/json' --data-binary "$body")
    case "$res" in *"[HTTP 200]") log "$name: 反映しました" ;; *) log "$name: 登録に失敗 $res"; rc=1 ;; esac
  else
    log "$name: 管理トークンが無いためファイルのみ更新(次回の再起動で反映)"
  fi
done
# open-web-server が登録時にファイルを保存し直す場合があるため、最後に秘密鍵の権限を揃える
chmod 600 "$certdir"/*.key 2>/dev/null
exit $rc
