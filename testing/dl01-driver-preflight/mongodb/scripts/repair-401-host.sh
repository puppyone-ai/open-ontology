#!/bin/bash
# One real upstream 401. The proxy opens 8317 and replaces the bearer.
# It does not invent a status when the connect fails, and it does not log either bearer.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
BIN="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight/raw/repair-59/bin"
OUT="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/repair-59-401"
ADMIT="$SRC/scripts/admit.py"
RUST="oo-dl01w1-41-rust-401"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
TOKEN_FILE="/tmp/oo-customer-demo-20261007/llm-token"
PORT=18317

mkdir -p "$OUT"
docker image inspect "$RUST_IMAGE" >/dev/null
test -x "$BIN/loopback-proxy"
python3 "$ADMIT" acquire rust "$OUT/admission.json" > "$OUT/admission.stdout"
cleanup() {
  docker logs "$RUST" > "$OUT/proxy.log" 2> "$OUT/proxy.err" || true
  docker rm -f "$RUST" >/dev/null 2>&1 || true
  python3 "$ADMIT" release rust > "$OUT/release.json" 2>&1 || true
}
trap cleanup EXIT

docker rm -f "$RUST" >/dev/null 2>&1 || true
docker run -d --name "$RUST" \
  --platform linux/arm64 --pull never \
  --memory 512m --memory-swap 512m --cpus 1 \
  --pids-limit 128 \
  --add-host host.docker.internal:host-gateway \
  -p "127.0.0.1:${PORT}:18317" \
  -e OO_PROXY_MODE=401 \
  -e OO_UPSTREAM=host.docker.internal:8317 \
  -e OO_PROXY_BIND=0.0.0.0:18317 \
  -v "$BIN/loopback-proxy:/proxy/loopback-proxy:ro" \
  --label project=open-ontology \
  --label owner=dl01-w1-41 \
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
  --label task=DL01-W1-41 \
  --label kind=rust-builder \
  --entrypoint /proxy/loopback-proxy \
  "$RUST_IMAGE" > "$OUT/docker-run.txt"

python3 - "$PORT" "$OUT" "$TOKEN_FILE" << 'PY'
import json, socket, sys, time
from pathlib import Path
port = int(sys.argv[1])
out = Path(sys.argv[2])
token_path = Path(sys.argv[3])
canary = b"CANARY-SECRET-VALUE"
body = b"{}"
request = (
    b"POST /v1/chat/completions HTTP/1.1\r\n"
    b"Host: 127.0.0.1\r\n"
    b"Authorization: Bearer " + canary + b"\r\n"
    b"Content-Length: " + str(len(body)).encode() + b"\r\n"
    b"Connection: close\r\n\r\n" + body
)
deadline = time.time() + 20
sock = None
while time.time() < deadline:
    try:
        sock = socket.create_connection(("127.0.0.1", port), 2)
        break
    except OSError:
        time.sleep(0.2)
if sock is None:
    out.joinpath("client.json").write_text(json.dumps({"connected": False}) + "\n")
    raise SystemExit(2)
sock.settimeout(20)
sock.sendall(request)
chunks = []
while True:
    try:
        piece = sock.recv(4096)
    except socket.timeout:
        break
    if not piece:
        break
    chunks.append(piece)
raw = b"".join(chunks)
status = None
first = raw.split(b"\r\n", 1)[0].decode("utf-8", "replace")
parts = first.split()
if len(parts) >= 2 and parts[1].isdigit():
    status = int(parts[1])
report = {
    "connected": True,
    "provider_status": status,
    "status_line": first[:120],
    "canary_in_response": canary in raw,
    "response_bytes": len(raw),
}
token = token_path.read_bytes().strip() if token_path.is_file() else b""
report["token_in_response"] = bool(token) and token in raw
out.joinpath("client.json").write_text(json.dumps(report, indent=2) + "\n")
if report["canary_in_response"] or report["token_in_response"]:
    raise SystemExit(3)
PY

docker logs "$RUST" > "$OUT/proxy.log" 2> "$OUT/proxy.err" || true
python3 - "$OUT" "$TOKEN_FILE" << 'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
token = Path(sys.argv[2]).read_bytes().strip() if Path(sys.argv[2]).is_file() else b""
err = (out / "proxy.err").read_bytes()
log = (out / "proxy.log").read_bytes()
text = err.decode("utf-8", "replace")
canary = b"CANARY-SECRET-VALUE"
facts = {
    "upstream_socket_opened": "mode=401 upstream_socket=opened" in text,
    "upstream_not_opened": "mode=401 upstream_not_opened" in text,
    "synthetic_status_none": "synthetic_status=none" in text,
    "provider_status_line": next((line for line in text.splitlines() if "provider_status=" in line), ""),
    "connect_fail_line": next((line for line in text.splitlines() if "upstream_connect_fail" in line), ""),
    "canary_in_proxy_log": canary in err or canary in log,
    "token_in_proxy_log": bool(token) and (token in err or token in log),
    "authorization_logged": "Authorization" in text or "Bearer" in text,
}
(out / "proxy-facts.json").write_text(json.dumps(facts, indent=2) + "\n")
if facts["canary_in_proxy_log"] or facts["token_in_proxy_log"] or facts["authorization_logged"]:
    raise SystemExit(4)
if not facts["upstream_socket_opened"] and not facts["upstream_not_opened"]:
    raise SystemExit(5)
PY
echo "401-proof-recorded"
