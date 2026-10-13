#!/bin/bash
# Drive the real session-control-api. Cookies and the grant stay in the volume.
set -u
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run" "${STATE}/probe"
if ! docker ps -q --filter name='^oo-a395-session$' | grep -q .; then
  echo "session_not_running" > "${EVIDENCE}/run/session-exchange.txt"
  printf '%s\n' 2 > "${EVIDENCE}/run/session-exchange.exit"
  exit 0
fi
python3 - "${STATE}" <<'PY'
import json, pathlib, sys
state = pathlib.Path(sys.argv[1])
session = json.loads((state / "config" / "session.json").read_text())
session["issue"] = {
    "principal_id": "fixture-admin",
    "project_id": "alpha",
    "grant_out": "/state/grant.hex",
}
path = state / "config" / "issue.json"
path.write_text(json.dumps(session))
path.chmod(0o600)
PY
load_profiles
set +e
docker run --rm --name oo-a395-session-grant \
  --platform linux/arm64 \
  --pull never \
  --network "$NET_CTL" \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 512m --memory-swap 512m --cpus 0.5 --pids-limit 128 \
  -e HOME=/tmp \
  --mount "type=bind,source=${STATE}/config,target=/config,readonly" \
  --mount "type=volume,source=oo-a395-eef831aa-session,target=/state" \
  --label oo.project=open-ontology \
  --label oo.owner=a395 \
  --label oo.session="${UUID}" \
  --label oo.kind=session-grant \
  --entrypoint bash \
  oo-a395-session:local \
  -c 'exec /usr/local/bin/service issue-grant < /config/issue.json' > "${EVIDENCE}/run/session-grant.log" 2>&1
printf '%s\n' $? > "${EVIDENCE}/run/session-grant.exit"
docker exec oo-a395-session bash -c '
set -e
port=$(tr -d "\r\n" < /state/port.txt)
grant=$(tr -d "\r\n" < /state/grant.hex)
req() {
  method=$1
  path=$2
  out=$3
  body=$4
  cookie=$5
  csrf=$6
  {
    printf "%s %s HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n" "$method" "$path"
    if [ -n "$cookie" ]; then printf "Cookie: oo-entry=%s\r\n" "$cookie"; fi
    if [ -n "$csrf" ]; then printf "X-OO-CSRF: %s\r\n" "$csrf"; fi
    if [ -n "$body" ]; then
      printf "Content-Type: application/json\r\nContent-Length: %s\r\n" "${#body}"
    fi
    printf "\r\n%s" "$body"
  } > /state/req.bin
  exec 3<>"/dev/tcp/127.0.0.1/${port}"
  cat /state/req.bin >&3
  timeout 15 cat <&3 > "$out" || true
  exec 3<&-
  rm -f /state/req.bin
}
body=$(printf "{\"grant\":\"%s\"}" "$grant")
req POST /control/v1/entry/session/consume /state/consume.http "$body" "" ""
cookie=$(sed -n "s/.*oo-entry=\\([a-f0-9][a-f0-9]*\\).*/\\1/p" /state/consume.http | head -1)
csrf=$(sed -n "s/.*\"csrf\":\"\\([a-f0-9][a-f0-9]*\\)\".*/\\1/p" /state/consume.http | head -1)
req POST /control/v1/conversations /state/create.http "{}" "$cookie" "$csrf"
cid=$(sed -n "s/.*\"conversation_id\":\"\\([^\"]*\\)\".*/\\1/p" /state/create.http | head -1)
if [ -n "$cid" ]; then
  req POST "/control/v1/conversations/${cid}/cancel" /state/cancel.http "{}" "$cookie" "$csrf"
elif [ -s /state/create.http ]; then
  cp /state/create.http /state/cancel.http
else
  printf "%s\n" "no conversation" > /state/cancel.http
fi
rm -f /state/grant.hex /state/req.bin
: > /state/cookie.wipe
' > "${EVIDENCE}/run/session-http.log" 2>&1
printf '%s\n' $? > "${EVIDENCE}/run/session-exchange.exit"
set -e
docker exec oo-a395-session bash -c 'rm -f /state/grant.hex /state/req.bin /state/cookie.wipe' >/dev/null 2>&1 || true
for name in consume create cancel; do
  docker cp "oo-a395-session:/state/${name}.http" "${STATE}/probe/${name}.http" >/dev/null 2>&1 || true
done
echo "session_exchange_done"
