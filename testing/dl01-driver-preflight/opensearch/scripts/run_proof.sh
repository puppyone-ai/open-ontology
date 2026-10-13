#!/bin/bash
# Start one owned OpenSearch 3.9.0, then one runner. Does not start PostgreSQL.
set -euo pipefail
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/opensearch"
EVIDENCE="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight"
IMAGE="opensearchproject/opensearch@sha256:cf05462a958be93fe2172aea2c6757debc7d3a1e4a6838d39e56af591a3dcdbb"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
mkdir -p "$EVIDENCE/proof"

if docker ps --format '{{.Names}}' | grep -qx 'oo-dl01w1-44-rust'; then
  echo "rust builder is still running" >&2
  exit 1
fi
if [ ! -x "$EVIDENCE/bin/os-reader" ] || [ ! -x "$EVIDENCE/bin/os-decode" ] || [ ! -x "$EVIDENCE/bin/os-judge" ]; then
  echo "linux binaries are missing" >&2
  exit 1
fi
if docker ps -a --format '{{.Names}}' | grep -qx 'oo-dl01w1-44-rust'; then
  owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' oo-dl01w1-44-rust)"
  status="$(docker inspect -f '{{.State.Status}}' oo-dl01w1-44-rust)"
  if [ "$owner" != "oo-dl01w1-44" ] || [ "$status" = "running" ]; then
    echo "refusing to remove rust container" >&2
    exit 1
  fi
  docker rm oo-dl01w1-44-rust >/dev/null
fi
python3 "$SRC/scripts/admit.py" release rust >/dev/null

cpu_room() {
  python3 - <<'PY'
import subprocess, sys
ids = [line for line in subprocess.check_output(["docker", "ps", "-q"], text=True).splitlines() if line]
total = 0
if ids:
    inspected = subprocess.check_output(["docker", "inspect", "-f", "{{.HostConfig.NanoCpus}}", *ids], text=True)
    for line in inspected.splitlines():
        total += int(line.strip() or "0")
sys.exit(0 if total / 1_000_000_000 + 1 <= 16 else 3)
PY
}

admit_kind() {
  kind="$1"
  dest="$2"
  set +e
  python3 "$SRC/scripts/admit.py" acquire "$kind" "$dest"
  code=$?
  if [ "$code" != "0" ]; then
    echo "capacity wait 900s for $kind" >&2
    sleep 900
    python3 "$SRC/scripts/admit.py" acquire "$kind" "$dest"
    code=$?
  fi
  set -e
  return "$code"
}

admit_kind source "$EVIDENCE/proof/admission-source.json"
python3 "$SRC/scripts/render_private.py" >"$EVIDENCE/proof/render.stdout" 2>"$EVIDENCE/proof/render.stderr"
docker network inspect oo-dl01w1-44-net >/dev/null 2>&1 || docker network create oo-dl01w1-44-net >/dev/null
docker volume rm -f oo-dl01w1-44-data >/dev/null 2>&1 || true
docker volume create oo-dl01w1-44-data >/dev/null
set +e
cpu_room
room=$?
if [ "$room" != "0" ]; then
  echo "capacity wait 900s before opensearch" >&2
  sleep 900
  cpu_room
  room=$?
fi
set -e
if [ "$room" != "0" ]; then
  echo "no CPU room for opensearch" >&2
  exit 3
fi
docker run -d --name oo-dl01w1-44-opensearch --pull never \
  --network oo-dl01w1-44-net \
  --memory 2g --memory-swap 2g --cpus 1 \
  --ulimit nofile=65536:65536 \
  -p 127.0.0.1:19244:9200 \
  -v /tmp/oo-dl01w1-44-private/opensearch.yml:/usr/share/opensearch/config/opensearch.yml:ro \
  -v /tmp/oo-dl01w1-44-private/certs:/usr/share/opensearch/config/certs:ro \
  -v /tmp/oo-dl01w1-44-private/security:/usr/share/opensearch/config/opensearch-security:ro \
  -v oo-dl01w1-44-data:/usr/share/opensearch/data \
  -e discovery.type=single-node \
  -e DISABLE_INSTALL_DEMO_CONFIG=true \
  -e DISABLE_PERFORMANCE_ANALYZER_AGENT_CLI=true \
  -e 'OPENSEARCH_JAVA_OPTS=-Xms512m -Xmx512m' \
  --label project=open-ontology \
  --label owner=oo-dl01w1-44 \
  --label session=bc6cf794-2d14-4e98-bcb6-f7a9b3533a88 \
  --label kind=source \
  --label task=DL01-W1-44 \
  "$IMAGE" >/dev/null

python3 - <<'PY'
import base64, json, ssl, time, urllib.request
from pathlib import Path
priv = Path("/tmp/oo-dl01w1-44-private")
pairs = {}
for line in (priv / "identities.env").read_text(encoding="utf-8").splitlines():
    if "=" in line:
        key, value = line.split("=", 1)
        pairs[key] = value
token = base64.b64encode(f"{pairs['dba_user']}:{pairs['dba_password']}".encode()).decode()
ctx = ssl.create_default_context(cafile=str(priv / "certs" / "ca.pem"))
last = "not-ready"
out = Path("/Users/redux/puppy-one/open-ontology/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/proof/health.json")
for _ in range(60):
    try:
        request = urllib.request.Request("https://127.0.0.1:19244/", headers={"Authorization": f"Basic {token}"})
        with urllib.request.urlopen(request, context=ctx, timeout=5) as response:
            body = json.loads(response.read().decode("utf-8"))
            status = response.getcode()
        version = body.get("version", {})
        fact = {"status": status, "distribution": version.get("distribution"), "number": version.get("number")}
        out.write_text(json.dumps(fact) + "\n", encoding="utf-8")
        if fact["distribution"] == "opensearch" and fact["number"] == "3.9.0" and status == 200:
            raise SystemExit(0)
        last = "identity"
    except SystemExit:
        raise
    except Exception as error:
        last = type(error).__name__
    time.sleep(5)
raise SystemExit(f"opensearch did not become ready: {last}")
PY

admit_kind rust "$EVIDENCE/proof/admission-runner.json"
docker run -d --name oo-dl01w1-44-runner \
  --platform linux/arm64 --pull never \
  --network oo-dl01w1-44-net \
  --memory 2g --memory-swap 2g --cpus 1 --pids-limit 256 \
  --security-opt no-new-privileges --cap-drop ALL \
  --user 0:0 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "$EVIDENCE/bin:/bins:ro" \
  -v /tmp/oo-dl01w1-44-private:/private:ro \
  -v "$SRC/fixtures:/src/fixtures:ro" \
  -v "$SRC/scripts/proof_inside.sh:/proof.sh:ro" \
  -v "$EVIDENCE:/evidence" \
  --label project=open-ontology \
  --label owner=oo-dl01w1-44 \
  --label session=bc6cf794-2d14-4e98-bcb6-f7a9b3533a88 \
  --label kind=rust-runner \
  --label task=DL01-W1-44 \
  "$RUST_IMAGE" \
  bash /proof.sh >/dev/null
echo "runner_started=1"
