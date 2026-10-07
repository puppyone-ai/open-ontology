#!/bin/bash
# One restart, then one status observation. No polling loop in the session.
set -euo pipefail
OWNER=a359
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a359
FIXED=a13b1cc80ed66c07199a41d47b31fd42530f2be9
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE="$REPO/evidence/${RUN_ID}/SP08/release-coordinator"
OUT="$EVIDENCE/linux/restart-status"
ROOT=/tmp/oo-a359-${RUN_ID}
SEC="$ROOT/secrets"
NET=oo-a359-eef831aa-net
PG_HOST=oo-a359-pg
TARGET_VOL=oo-a359-eef831aa-target
NAME=oo-a359-build
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5

mkdir -p "$OUT"
if docker ps -q --filter name="^${NAME}$" | grep -q .; then
  echo "builder still running" | tee "$OUT/refused.txt"
  exit 2
fi
if [ ! -f "$EVIDENCE/observation.json" ]; then
  echo "observation missing" | tee "$OUT/refused.txt"
  exit 2
fi
own=$(docker ps -q --filter label=oo.kind=rust-builder --filter label=oo.owner="$OWNER" | wc -l | tr -d ' ')
global=$(docker ps -q --filter label=oo.kind=rust-builder | wc -l | tr -d ' ')
host_tools=$(ps ax -o command= | awk 'BEGIN{n=0} $1 ~ /(^|\/)(cargo|rustc|rustdoc|rustup)$/ {n++} END{print n+0}')
printf 'own=%s global=%s host_tools=%s\n' "$own" "$global" "$host_tools" | tee "$OUT/capacity.txt"
if [ "$own" != 0 ] || [ "$global" -ge 10 ] || [ "$host_tools" != 0 ]; then
  echo "capacity refused" >&2
  exit 2
fi

python3 - << 'PY' > "$OUT/status-stdin.json"
import json, pathlib
obs = json.loads(pathlib.Path("/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a359/SP08/release-coordinator/observation.json").read_text())
doc = {
    "protocol": "oo.release-coordinator/status/v1",
    "project_id": "shop",
    "candidate_id": obs["candidate_id"],
    "source_hash": obs["source_hash"],
}
pathlib.Path("/tmp/oo-a359-status-stdin.json").write_text(json.dumps(doc))
print(json.dumps({"wrote": "status-stdin", "candidate_id": obs["candidate_id"], "pointer_version": obs["pointer_version"]}))
PY
cp /tmp/oo-a359-status-stdin.json "$OUT/status-stdin.json"
python3 - << 'PY'
import json
cfg = {
    "host": "oo-a359-pg",
    "port": 5432,
    "database": "oo_a359",
    "user": "oo_release_admin",
    "password_file": "/secrets/admin.pass",
    "ca_file": "/secrets/ca.crt",
}
open("/tmp/oo-a359-status-config.json","w").write(json.dumps(cfg))
PY
cp /tmp/oo-a359-status-config.json "$OUT/status-config.json"

docker restart oo-a359-pg > "$OUT/restart.out"
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
  if docker exec -u postgres oo-a359-pg pg_isready -U oo_fixture_admin -d oo_a359 > "$OUT/pg-isready.txt" 2>&1; then
    ready=1
    break
  fi
  sleep 5
done
if [ "$ready" != 1 ]; then
  echo "postgres not ready" | tee "$OUT/refused.txt"
  exit 1
fi
docker inspect oo-a359-pg --format 'status={{.State.Status}} oom={{.State.OOMKilled}} started={{.State.StartedAt}}' > "$OUT/pg-after-restart.txt"

set +e
docker run --name "$NAME" -i \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${OUT},target=/work,readonly" \
  --mount "type=volume,source=${TARGET_VOL},target=/target,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
  --label oo.project=open-ontology \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.fixed="$FIXED" \
  --label oo.kind=rust-builder \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  /target/debug/release-coordinator status --config /work/status-config.json < "$OUT/status-stdin.json" > "$OUT/stdout.json" 2> "$OUT/stderr.txt"
status_exit=$?
set -e
printf '%s\n' "$status_exit" > "$OUT/exit.txt"
docker inspect "$NAME" --format 'oom={{.State.OOMKilled}} exit={{.State.ExitCode}} privileged={{.HostConfig.Privileged}}' > "$OUT/hostconfig.txt" || true
docker rm "$NAME" >/dev/null || true
echo "STATUS_EXIT=$status_exit"
exit "$status_exit"
