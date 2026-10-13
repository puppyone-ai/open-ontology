#!/bin/bash
# Capture real HTTP bodies. A missing tool or a missing service stays not ready.
set -u
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run" "${STATE}/probe"
TOOLS=${ROOT}/evidence/${UUID}/SP14/product-stack/linux/build/tools.txt
has() { grep -q "/$1$" "$TOOLS" 2>/dev/null || grep -q "/$1" "$TOOLS" 2>/dev/null; }

note() {
  local name="$1" http="$2" tcp="$3" body="$4"
  python3 - "$name" "$http" "$tcp" "$body" "${STATE}/probe/${name}.json" <<'PY'
import json, pathlib, sys
name, http, tcp, body, path = sys.argv[1:]
doc = {"name": name, "http": int(http), "tcp_only": tcp == "true", "body": body}
pathlib.Path(path).write_text(json.dumps(doc))
PY
}

note catalog 0 false '{"error":"not_started","service":"source-control-api","status":"recipe_only","gap_owner":"a403","binary_owner":"a383","reason":"head_schemas_only_implementation_untracked","not_a_producer":"a393"}'

if has curl && docker ps -q --filter name='^oo-a395-identity$' | grep -q .; then
  set +e
  docker run --rm --name oo-a395-probe-identity \
    --platform linux/arm64 --pull never --network "$NET_CTL" \
    --user 0:0 --security-opt no-new-privileges --cap-drop ALL \
    --memory 256m --memory-swap 256m --cpus 0.5 --pids-limit 64 \
    -e HOME=/tmp \
    --mount "type=bind,source=${STATE}/secrets/ca.crt,target=/ca.crt,readonly" \
    --label oo.owner=a395 --label oo.session="${UUID}" --label oo.kind=probe \
    "$RUST_IMAGE" \
    curl -sS --max-time 10 -w '\n%{http_code}' --cacert /ca.crt \
    https://control-identity:8443/control/v1/identity/current > "${STATE}/probe/oidc.raw" 2>"${STATE}/probe/oidc.err"
  printf '%s\n' $? > "${EVIDENCE}/run/oidc-probe.exit"
  set -e
  python3 - "${STATE}/probe/oidc.raw" "${STATE}/probe/oidc_current.json" <<'PY'
import json, pathlib, sys
raw = pathlib.Path(sys.argv[1]).read_text(errors="replace")
lines = raw.splitlines()
code = 0
body = raw
if lines and lines[-1].isdigit():
    code = int(lines[-1])
    body = "\n".join(lines[:-1])
if any(token in body for token in ("PRIVATE KEY", "BEGIN ", "password")):
    body = "{}"
try:
    json.loads(body)
except Exception:
    body = json.dumps({"error": "non_json"})
pathlib.Path(sys.argv[2]).write_text(json.dumps({"name":"oidc_current","http":code,"tcp_only":False,"body":body}))
PY
else
  note oidc_current 0 false '{"error":"probe_tool_absent_or_service_down"}'
  printf '%s\n' 2 > "${EVIDENCE}/run/oidc-probe.exit"
fi

if has openssl && docker ps -q --filter name='^oo-a395-audit$' | grep -q .; then
  python3 - "${STATE}/probe/audit.req" <<'PY'
import hashlib, json, pathlib, uuid, sys
payload = b"oo-a395"
body = {
    "protocol": "oo.audit.ingest/v1",
    "spool_id": str(uuid.uuid4()),
    "request_id": str(uuid.uuid4()),
    "event_seq": 1,
    "payload_hash": hashlib.sha256(payload).hexdigest(),
    "payload_hex": payload.hex(),
    "project": "alpha",
    "kind": "admission",
}
raw = json.dumps(body, separators=(",", ":")).encode()
request = b"POST /v1/audit-events HTTP/1.1\r\nHost: audit-ingest\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: " + str(len(raw)).encode() + b"\r\n\r\n" + raw
pathlib.Path(sys.argv[1]).write_bytes(request)
PY
  set +e
  docker run --rm --name oo-a395-probe-audit \
    --platform linux/arm64 --pull never --network "$NET_CTL" \
    --user 0:0 --security-opt no-new-privileges --cap-drop ALL \
    --memory 256m --memory-swap 256m --cpus 0.5 --pids-limit 64 \
    -e HOME=/tmp \
    --mount "type=bind,source=${STATE}/secrets/ca.crt,target=/ca.crt,readonly" \
    --mount "type=bind,source=${STATE}/secrets/audit-client.crt,target=/client.crt,readonly" \
    --mount "type=bind,source=${STATE}/secrets/audit-client.key,target=/client.key,readonly" \
    --mount "type=bind,source=${STATE}/probe/audit.req,target=/req,readonly" \
    --label oo.owner=a395 --label oo.session="${UUID}" --label oo.kind=probe \
    "$RUST_IMAGE" \
    sh -c 'timeout 20 openssl s_client -connect audit-ingest:8444 -servername audit-ingest -cert /client.crt -key /client.key -CAfile /ca.crt -verify_hostname audit-ingest -quiet -ign_eof < /req' > "${STATE}/probe/audit.raw" 2>"${STATE}/probe/audit.err"
  printf '%s\n' $? > "${EVIDENCE}/run/audit-probe.exit"
  set -e
  python3 - "${STATE}/probe/audit.raw" "${STATE}/probe/audit_ack.json" <<'PY'
import json, pathlib, sys
raw = pathlib.Path(sys.argv[1]).read_text(errors="replace")
marker = raw.find("HTTP/")
if marker > 0:
    raw = raw[marker:]
sep = "\r\n\r\n" if "\r\n\r\n" in raw else "\n\n"
parts = raw.split(sep, 1) if sep in raw else [raw]
header = parts[0].replace("\r", "")
body = parts[1].strip() if len(parts) == 2 else raw.strip()
code = 0
if header.startswith("HTTP/"):
    bits = header.split()
    if len(bits) > 1 and bits[1].isdigit():
        code = int(bits[1])
if any(token in body for token in ("PRIVATE KEY", "BEGIN ")):
    body = "{}"
try:
    json.loads(body)
except Exception:
    body = json.dumps({"error": "non_json"})
pathlib.Path(sys.argv[2]).write_text(json.dumps({"name":"audit_ack","http":code,"tcp_only":False,"body":body}))
PY
else
  note audit_ack 0 false '{"error":"probe_tool_absent_or_service_down"}'
  printf '%s\n' 2 > "${EVIDENCE}/run/audit-probe.exit"
fi

if docker ps -q --filter name='^oo-a395-release$' | grep -q .; then
  set +e
  docker run --rm --name oo-a395-probe-release \
    --platform linux/arm64 --pull never --network "$NET_CTL" \
    --user 0:0 --security-opt no-new-privileges --cap-drop ALL \
    --memory 256m --memory-swap 256m --cpus 0.5 --pids-limit 64 \
    -e HOME=/tmp \
    --mount "type=bind,source=${STATE}/secrets/ca.crt,target=/ca.crt,readonly" \
    --label oo.owner=a395 --label oo.session="${UUID}" --label oo.kind=probe \
    "$RUST_IMAGE" \
    curl -sS --max-time 10 -w '\n%{http_code}' --cacert /ca.crt \
    'https://release-control-api:8446/control/v1/releases/active?project_id=alpha' > "${STATE}/probe/release.raw" 2>"${STATE}/probe/release.err"
  printf '%s\n' $? > "${EVIDENCE}/run/release-probe.exit"
  set -e
  python3 - "${STATE}/probe/release.raw" "${STATE}/probe/release.json" <<'PY'
import json, pathlib, sys
raw = pathlib.Path(sys.argv[1]).read_text(errors="replace")
lines = raw.splitlines()
code = 0
body = raw
if lines and lines[-1].isdigit():
    code = int(lines[-1])
    body = "\n".join(lines[:-1])
if any(token in body for token in ("PRIVATE KEY", "BEGIN ", "password")):
    body = "{}"
try:
    json.loads(body)
except Exception:
    body = json.dumps({"error": "non_json"})
pathlib.Path(sys.argv[2]).write_text(json.dumps({"name":"release","http":code,"tcp_only":False,"body":body}))
PY
else
  note release 0 false '{"error":"not_started","service":"release-control-api","status":"container_absent"}'
  printf '%s\n' 2 > "${EVIDENCE}/run/release-probe.exit"
fi

if has curl && docker ps -q --filter name='^oo-a395-selfbuild$' | grep -q .; then
  set +e
  docker run --rm --name oo-a395-probe-selfbuild \
    --platform linux/arm64 --pull never --network "$NET_CTL" \
    --user 0:0 --security-opt no-new-privileges --cap-drop ALL \
    --memory 256m --memory-swap 256m --cpus 0.5 --pids-limit 64 \
    -e HOME=/tmp \
    --mount "type=bind,source=${STATE}/secrets/ca.crt,target=/ca.crt,readonly" \
    --label oo.owner=a395 --label oo.session="${UUID}" --label oo.kind=probe \
    "$RUST_IMAGE" \
    curl -sS --max-time 10 -w '\n%{http_code}' --cacert /ca.crt \
    'https://selfbuild-control-api:8447/control/v1/builds/b_00000000000000000000000000000000/pack' > "${STATE}/probe/pack.raw" 2>"${STATE}/probe/pack.err"
  printf '%s\n' $? > "${EVIDENCE}/run/pack-probe.exit"
  set -e
  python3 - "${STATE}/probe/pack.raw" "${STATE}/probe/pack.json" <<'PY'
import json, pathlib, sys
raw = pathlib.Path(sys.argv[1]).read_text(errors="replace")
lines = raw.splitlines()
code = 0
body = raw
if lines and lines[-1].isdigit():
    code = int(lines[-1])
    body = "\n".join(lines[:-1])
if any(token in body for token in ("PRIVATE KEY", "BEGIN ", "password")):
    body = "{}"
try:
    json.loads(body)
except Exception:
    body = json.dumps({"error": "non_json"})
pathlib.Path(sys.argv[2]).write_text(json.dumps({"name":"pack","http":code,"tcp_only":False,"body":body}))
PY
  docker exec oo-a395-selfbuild sh -c 'for p in /producers/catalog /producers/pg-builder /producers/rules /producers/suggestions /producers/review-ledger; do if [ -f "$p" ]; then echo "$p file"; else echo "$p absent"; fi; done' > "${EVIDENCE}/run/producer-bins.txt" 2>/dev/null || true
else
  note pack 0 false '{"error":"not_started","service":"selfbuild-control-api","status":"container_absent","reason":"producer_binaries_absent","gap_owner":"a373"}'
  printf '%s\n' 2 > "${EVIDENCE}/run/pack-probe.exit"
fi

"$(dirname "$0")/session-exchange.sh" || true
if [ -f "${STATE}/probe/cancel.http" ]; then
  python3 - "${STATE}/probe/cancel.http" "${STATE}/probe/session_close.json" <<'PY'
import json, pathlib, sys
raw = pathlib.Path(sys.argv[1]).read_text(errors="replace")
sep = "\r\n\r\n" if "\r\n\r\n" in raw else "\n\n"
parts = raw.split(sep, 1) if sep in raw else [raw]
header = parts[0].replace("\r", "")
body = parts[1].strip() if len(parts) == 2 else raw.strip()
code = 0
if header.startswith("HTTP/"):
    bits = header.split()
    if len(bits) > 1 and bits[1].isdigit():
        code = int(bits[1])
if any(token in body for token in ("PRIVATE KEY", "BEGIN ", "oo-entry=", "password")):
    body = "{}"
try:
    json.loads(body)
except Exception:
    body = json.dumps({"error": "non_json"})
pathlib.Path(sys.argv[2]).write_text(json.dumps({"name":"session_close","http":code,"tcp_only":False,"body":body}))
PY
else
  note session_close 0 false '{"error":"session_exchange_absent"}'
fi
echo "health_captured"
