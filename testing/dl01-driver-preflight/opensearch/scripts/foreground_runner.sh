#!/bin/bash
# The OpenSearch container is already running. Wait for one free CPU, then start
# the named runner and wait until that container exits. A launcher exit is not the proof.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/opensearch"
EVIDENCE="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
mkdir -p "$EVIDENCE/proof/failures"

if ! docker ps --format '{{.Names}}' | grep -qx 'oo-dl01w1-44-opensearch'; then
  echo "OWN_OPENSEARCH_NOT_RUNNING"
  exit 1
fi
owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' oo-dl01w1-44-opensearch)"
if [ "$owner" != "oo-dl01w1-44" ]; then
  echo "refusing foreign opensearch"
  exit 1
fi
if docker ps -a --format '{{.Names}}' | grep -qx 'oo-dl01w1-44-runner'; then
  runner_owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' oo-dl01w1-44-runner)"
  runner_status="$(docker inspect -f '{{.State.Status}}' oo-dl01w1-44-runner)"
  if [ "$runner_owner" != "oo-dl01w1-44" ] || [ "$runner_status" = "running" ]; then
    echo "RUNNER_ALREADY_PRESENT"
    docker ps -a --filter name=oo-dl01w1-44-runner --format '{{.Names}} {{.Status}}'
    exit 1
  fi
  docker rm oo-dl01w1-44-runner >/dev/null
fi
if [ -f "$EVIDENCE/proof/admission-runner.json" ]; then
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  cp "$EVIDENCE/proof/admission-runner.json" "$EVIDENCE/proof/failures/admission-runner-${stamp}.json"
fi

try_start() {
  python3 "$SRC/scripts/admit.py" acquire rust "$EVIDENCE/proof/admission-runner.json"
  code=$?
  if [ "$code" != "0" ]; then
    return "$code"
  fi
  if ! docker run -d --name oo-dl01w1-44-runner \
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
    bash /proof.sh >/dev/null; then
    python3 "$SRC/scripts/admit.py" release rust >/dev/null || true
    echo "docker run runner failed; rust slot released" >&2
    return 1
  fi
  return 0
}

echo "runner_try $(date -u +%Y-%m-%dT%H:%M:%SZ)"
set +e
try_start
start_code=$?
set -e
if [ "$start_code" != "0" ]; then
  echo "start_exit=${start_code} runner_capacity_wait_900_begin $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  sleep 900
  echo "runner_capacity_wait_900_end $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  set +e
  try_start
  start_code=$?
  set -e
fi
if [ "$start_code" != "0" ]; then
  echo "start_exit=${start_code} second_capacity_wait_900_begin $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  sleep 900
  echo "second_capacity_wait_900_end $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  set +e
  try_start
  start_code=$?
  set -e
  if [ "$start_code" != "0" ]; then
    echo "start_exit=${start_code} runner_not_started"
    docker ps --format '{{.Names}}' | wc -l
    exit "$start_code"
  fi
fi

echo "RUNNER_STARTED $(date -u +%Y-%m-%dT%H:%M:%SZ)"
runner_exit="$(docker wait oo-dl01w1-44-runner | tail -n 1)"
echo "DOCKER_PHASE=exited runner_exit=${runner_exit} $(date -u +%Y-%m-%dT%H:%M:%SZ)"
docker inspect -f 'status={{.State.Status}} exit={{.State.ExitCode}} started={{.State.StartedAt}} finished={{.State.FinishedAt}} oom={{.State.OOMKilled}} error={{.State.Error}}' oo-dl01w1-44-runner
docker logs oo-dl01w1-44-runner >"$EVIDENCE/proof/runner.stdout" 2>"$EVIDENCE/proof/runner.stderr" || true
python3 "$SRC/scripts/admit.py" release rust >/dev/null || true
python3 - <<'PY'
import json
from pathlib import Path
root = Path("/Users/redux/puppy-one/open-ontology/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/proof")
for path in sorted(root.glob("*/exit")):
    print(f"{path.parent.name}={path.read_text().strip()}")
verdict = root / "judge" / "verdict.json"
print(f"verdict_present={verdict.exists()}")
print(f"finished={(root / 'finished').exists()}")
if verdict.exists():
    body = json.loads(verdict.read_text())
    print("verdict_keys=" + ",".join(sorted(body.keys())))
    failures = body.get("failures") or []
    print("failure_count=" + str(len(failures)))
    for item in failures[:30]:
        print("failure=" + str(item)[:400])
decide = root / "decide" / "exit"
print("decide_exit=" + (decide.read_text().strip() if decide.exists() else "MISSING"))
PY
exit "$runner_exit"
