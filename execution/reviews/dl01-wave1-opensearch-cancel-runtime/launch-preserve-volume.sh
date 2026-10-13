#!/bin/bash
# Launch the owned OpenSearch node and the user-cancel proof. Does not touch foreign containers.
set -euo pipefail
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/opensearch"
BUILD="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/build/user-cancel-04a5dda5"
PROOF="$ROOT/execution/reviews/dl01-wave1-opensearch-cancel-runtime"
IMAGE="opensearchproject/opensearch@sha256:cf05462a958be93fe2172aea2c6757debc7d3a1e4a6838d39e56af591a3dcdbb"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
if [ "$(cat "$BUILD/build/build.exit")" != "0" ]; then
  echo "build is not green" >&2
  exit 1
fi
if docker ps --format '{{.Names}}' | grep -qx 'oo-dl01w1-44-rust'; then
  echo "rust builder still running" >&2
  exit 1
fi
mkdir -p "$PROOF/proof"
python3 "$SRC/scripts/admit.py" release rust >/dev/null
python3 "$SRC/scripts/admit.py" acquire source "$PROOF/proof/admission-source.json"
if [ ! -d /tmp/oo-dl01w1-44-private ]; then
 python3 "$SRC/scripts/render_private.py" >"$PROOF/proof/render.stdout" 2>"$PROOF/proof/render.stderr"
fi
docker network inspect oo-dl01w1-44-net >/dev/null 2>&1 || docker network create oo-dl01w1-44-net >/dev/null
test -z "$(docker volume ls --filter name=oo-dl01w1-44-usercancel-20261011 --format '{{.Name}}')"
docker volume create oo-dl01w1-44-usercancel-20261011 >/dev/null
docker run -d --name oo-dl01w1-44-opensearch --pull never \
  --network oo-dl01w1-44-net \
  --memory 2g --memory-swap 2g --cpus 1 \
  --ulimit nofile=65536:65536 \
  -p 127.0.0.1:19244:9200 \
  -v /tmp/oo-dl01w1-44-private/opensearch.yml:/usr/share/opensearch/config/opensearch.yml:ro \
  -v /tmp/oo-dl01w1-44-private/certs:/usr/share/opensearch/config/certs:ro \
  -v /tmp/oo-dl01w1-44-private/security:/usr/share/opensearch/config/opensearch-security:ro \
  -v oo-dl01w1-44-usercancel-20261011:/usr/share/opensearch/data \
  -e discovery.type=single-node \
  -e DISABLE_INSTALL_DEMO_CONFIG=true \
  -e DISABLE_PERFORMANCE_ANALYZER_AGENT_CLI=true \
  -e 'OPENSEARCH_JAVA_OPTS=-Xms512m -Xmx512m' \
  --label project=open-ontology \
  --label owner=oo-dl01w1-44 \
  --label session=04a5dda5-7f08-45ea-a91b-298afba68cba \
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
out = Path("/Users/redux/puppy-one/open-ontology/execution/reviews/dl01-wave1-opensearch-cancel-runtime/proof/health.json")
last = "not-ready"
for _ in range(60):
    try:
        request = urllib.request.Request("https://127.0.0.1:19244/", headers={"Authorization": f"Basic {token}"})
        with urllib.request.urlopen(request, context=ctx, timeout=5) as response:
            body = json.loads(response.read().decode("utf-8"))
            code = response.getcode()
        version = body.get("version", {})
        fact = {"status": code, "distribution": version.get("distribution"), "number": version.get("number")}
        out.write_text(json.dumps(fact) + "\n", encoding="utf-8")
        if fact["distribution"] == "opensearch" and fact["number"] == "3.9.0" and code == 200:
            raise SystemExit(0)
        last = "identity"
    except SystemExit:
        raise
    except Exception as error:
        last = type(error).__name__
    time.sleep(5)
raise SystemExit(f"opensearch did not become ready: {last}")
PY
python3 "$SRC/scripts/admit.py" acquire rust "$PROOF/proof/admission-runner.json"
docker run -d --name oo-dl01w1-44-runner \
  --platform linux/arm64 --pull never \
  --network oo-dl01w1-44-net \
  --memory 2g --memory-swap 2g --cpus 1 --pids-limit 256 \
  --security-opt no-new-privileges --cap-drop ALL \
  --user 0:0 \
  -v "$BUILD/bin:/bins:ro" \
  -v /tmp/oo-dl01w1-44-private:/private:ro \
  -v "$SRC/fixtures:/src/fixtures:ro" \
  -v "$SRC/scripts/user_cancel_inside.sh:/proof.sh:ro" \
  -v "$PROOF:/evidence" \
  --label project=open-ontology \
  --label owner=oo-dl01w1-44 \
  --label session=04a5dda5-7f08-45ea-a91b-298afba68cba \
  --label kind=rust-runner \
  --label task=DL01-W1-44 \
  "$RUST_IMAGE" \
  bash /proof.sh >/dev/null
echo runner_started=1
