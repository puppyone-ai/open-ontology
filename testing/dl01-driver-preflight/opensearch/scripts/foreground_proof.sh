#!/bin/bash
# Start the owned OpenSearch proof and wait until the named runner exits.
# run_proof.sh returning is only the launch. The runner exit is the phase result.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/opensearch"
EVIDENCE="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight"
mkdir -p "$EVIDENCE/proof"

if docker ps --format '{{.Names}}' | grep -Eq '^(oo-dl01w1-44-runner|oo-dl01w1-44-opensearch|oo-dl01w1-44-rust)$'; then
  echo "ACTIVE_OWN_CONTAINER"
  docker ps -a --filter name=oo-dl01w1-44 --format '{{.Names}} {{.Status}}'
  exit 1
fi

echo "foreground_proof_start $(date -u +%Y-%m-%dT%H:%M:%SZ)"
set +e
bash "$SRC/scripts/run_proof.sh"
start_code=$?
set -e
echo "run_proof_exit=${start_code} $(date -u +%Y-%m-%dT%H:%M:%SZ)"
if ! docker ps -a --format '{{.Names}}' | grep -qx 'oo-dl01w1-44-runner'; then
  echo "NO_RUNNER"
  docker ps -a --filter name=oo-dl01w1-44 --format '{{.Names}} {{.Status}}' || true
  exit "$start_code"
fi
echo "RUNNER_WAIT $(date -u +%Y-%m-%dT%H:%M:%SZ)"
runner_exit="$(docker wait oo-dl01w1-44-runner | tail -n 1)"
echo "DOCKER_PHASE=exited runner_exit=${runner_exit} $(date -u +%Y-%m-%dT%H:%M:%SZ)"
docker inspect -f 'name={{.Name}} status={{.State.Status}} exit={{.State.ExitCode}} started={{.State.StartedAt}} finished={{.State.FinishedAt}} oom={{.State.OOMKilled}}' oo-dl01w1-44-runner
docker logs oo-dl01w1-44-runner >"$EVIDENCE/proof/runner.stdout" 2>"$EVIDENCE/proof/runner.stderr" || true
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
    print("verdict_outcome=" + str(body.get("outcome")))
    print("failure_count=" + str(len(body.get("failures") or [])))
    for item in (body.get("failures") or [])[:20]:
        print("failure=" + str(item)[:300])
decide = root / "decide" / "exit"
print("decide_exit=" + (decide.read_text().strip() if decide.exists() else "MISSING"))
PY
exit "$runner_exit"
