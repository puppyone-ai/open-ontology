#!/bin/bash
# Restart the owned database once, then one new process runs status.
# The binary is the archive build, not the review-test copy.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a366
REPO=/Users/redux/puppy-one/open-ontology
EVID=${REPO}/evidence/${RUN_ID}/SP08/release-coordinator-review
ROOT=/tmp/oo-a366-${RUN_ID}
OUT=${EVID}/linux/restart-status
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a366-status
PG=oo-a366-pg
NET=oo-a366-eef831aa-net
mkdir -p "$OUT"
global=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | wc -l | tr -d ' ')
own=$(docker ps -q --filter label=oo.owner=a366 --filter label=oo.kind=rust-builder --filter status=running | wc -l | tr -d ' ')
printf 'own=%s global=%s\n' "$own" "$global" | tee "$OUT/capacity.txt"
if [ "$global" -ge 10 ] || [ "$own" -ge 1 ]; then
  echo "builder capacity exceeded" >&2
  printf '%s\n' 3 > "$OUT/exit.txt"
  exit 3
fi
docker restart "$PG" > "$OUT/restart.out"
ready=0
i=0
while [ "$i" -lt 12 ]; do
  i=$((i + 1))
  if docker exec -u postgres "$PG" pg_isready -U oo_a366_admin -d oo_a366 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 5
done
docker exec -u postgres "$PG" pg_isready -U oo_a366_admin -d oo_a366 > "$OUT/pg-isready.txt" || true
if [ "$ready" -ne 1 ]; then
  echo "postgres did not accept connections after restart" >&2
  printf '%s\n' 1 > "$OUT/exit.txt"
  exit 1
fi
src=$(awk -F'"' '/"source_hash"/ {print $4; exit}' "$EVID/linux/build-1/observation.json")
test "${#src}" -eq 64
cat > "$OUT/status-config.json" <<'EOF'
{"host":"oo-a366-pg","port":5432,"database":"oo_a366","user":"oo_release_admin","password_file":"/secrets/release-admin.pass","ca_file":"/secrets/ca.crt"}
EOF
printf '%s\n' "{\"protocol\":\"oo.release-coordinator/status/v1\",\"project_id\":\"shop\",\"candidate_id\":\"rev-d\",\"source_hash\":\"${src}\"}" > "$OUT/status-stdin.json"
docker rm -f "$NAME" >/dev/null 2>&1 || true
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --network "$NET" \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  -e HOME=/tmp \
  -i \
  --mount "type=volume,source=oo-a366-target,target=/target,readonly" \
  --mount "type=bind,source=${ROOT}/secrets,target=/secrets,readonly" \
  --mount "type=bind,source=${OUT}/status-config.json,target=/config.json,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a366 \
  --label oo.session="$RUN_ID" \
  --label oo.kind=rust-builder \
  --label oo.task=release-coordinator-review-status \
  --entrypoint /target/product/debug/release-coordinator \
  "$IMAGE" status --config /config.json \
  < "$OUT/status-stdin.json" > "$OUT/stdout.json" 2> "$OUT/stderr.txt"
code=$?
set -e
printf '%s\n' "$code" > "$OUT/exit.txt"
docker inspect "$NAME" --format 'oom={{.State.OOMKilled}} exit={{.State.ExitCode}} privileged={{.HostConfig.Privileged}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$OUT/hostconfig.txt"
docker rm -f "$NAME" >/dev/null
exit "$code"
