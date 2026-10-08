#!/bin/bash
# Resume after attempt 01. Reuses the owned PostgreSQL, cargo volume, and archive.
# Does not start a second database or a second builder.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a400
FIXED=bb831355662c1dffd76b86bf60417158f4513134
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/SP02/secret-control-api-review
RAW=${ROOT}/linux/raw
SEC=/tmp/oo-a400-${RUN_ID}/secrets
STATE=/tmp/oo-a400-${RUN_ID}/state
ARCHIVE=/tmp/oo-a400-${RUN_ID}/archive
PROOF=/tmp/oo-a400-${RUN_ID}/proof
NET=oo-a400-eef831aa-net
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
cd "$REPO"

log() {
  printf '%s\n' "$1" >> "$RAW/launch.log"
}

own_rust=$(docker ps -q --filter label=oo.owner=a400 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_rust=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a400 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_rust=%s global_rust=%s host_cargo=%s own_pg=%s\n' "$own_rust" "$global_rust" "$host_cargo" "$own_pg" > "$RAW/capacity-resume.txt"
if [ "$own_rust" -ge 1 ] || [ "$global_rust" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ne 1 ]; then
  echo "deferred: resume capacity own_rust=$own_rust global_rust=$global_rust host_cargo=$host_cargo own_pg=$own_pg" >&2
  printf '%s\n' 3 > "$RAW/launch.exit"
  exit 3
fi
if [ ! -d "$ARCHIVE/tools/secret-control-api" ] || [ ! -d "$PROOF" ] || [ ! -d "$SEC" ] || [ ! -d "$STATE" ]; then
  echo "resume state is missing" >&2
  printf '%s\n' 2 > "$RAW/launch.exit"
  exit 2
fi

docker exec -u postgres oo-a400-pg psql -U oo_fixture_admin -d identity -v ON_ERROR_STOP=1 -c 'DROP SCHEMA IF EXISTS oo_identity_control CASCADE' > "$RAW/reset-identity.txt"
docker exec -u postgres oo-a400-pg psql -U oo_fixture_admin -d secret -v ON_ERROR_STOP=1 -c 'DROP SCHEMA IF EXISTS oo_secret_control CASCADE' > "$RAW/reset-secret.txt"
cp "$ROOT/overlay/independent/main.rs" "$ROOT/overlay/independent/idp.rs" "$PROOF/tests/independent/"
python3 - <<PY
import hashlib, pathlib, sys
archive = pathlib.Path("$ARCHIVE/tools/secret-control-api")
proof = pathlib.Path("$PROOF")
def rows(root, skip):
    out = []
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(root).as_posix()
        if rel.startswith(skip):
            continue
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        out.append(f"{digest}  ./{rel}")
    return out
left = rows(archive, "tests/independent/")
right = rows(proof, "tests/independent/")
if left != right:
    sys.stderr.write("archive and proof product bytes differ\n")
    sys.exit(1)
pathlib.Path("$RAW/archive-secret.sha256").write_text("\n".join(left) + "\n")
lines = []
for name in ("main.rs", "idp.rs"):
    file = proof / "tests/independent" / name
    lines.append(hashlib.sha256(file.read_bytes()).hexdigest() + "  tests/independent/" + name)
pathlib.Path("$RAW/overlay-sha256.txt").write_text("\n".join(lines) + "\n")
print("archive_match=yes")
PY

start_builder() {
  phase=$1
  docker rm -f oo-a400-proof >/dev/null 2>&1 || true
  docker run -d --name oo-a400-proof \
    --platform linux/arm64 \
    --pull never \
    --init \
    --security-opt no-new-privileges:true \
    --cap-drop ALL \
    --memory 6g \
    --memory-swap 6g \
    --cpus 1 \
    --pids-limit 1024 \
    --network "$NET" \
    --network-alias oo-a400-proof \
    -e PATH=/usr/local/cargo/bin:/usr/bin:/bin \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    -e CARGO_HOME=/cargo \
    -e CARGO_TARGET_DIR=/work/secret \
    -e OO_A400_PHASE="$phase" \
    --mount "type=bind,source=${PROOF},target=/src,readonly" \
    --mount "type=bind,source=${ARCHIVE}/tools/control-identity,target=/identity,readonly" \
    --mount "type=bind,source=${ROOT}/linux,target=/evidence/linux" \
    --mount "type=bind,source=${SEC},target=/secrets,readonly" \
    --mount "type=bind,source=${STATE},target=/state" \
    --mount "type=volume,source=oo-a400-cargo,target=/cargo" \
    --mount "type=volume,source=oo-a400-work,target=/work" \
    --label oo.project=open-ontology \
    --label oo.owner=a400 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=rust-builder \
    --label oo.task=secret-control-api-review \
    --entrypoint /evidence/linux/run-proof.sh \
    "$RUST_IMAGE" > "$RAW/builder-${phase}.id"
  docker inspect oo-a400-proof --format 'Status={{.State.Status}} OOM={{.HostConfig.Memory}} Cpu={{.HostConfig.NanoCpus}} Priv={{.HostConfig.Privileged}} Net={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} User={{.Config.User}}' > "$RAW/builder-${phase}-hostconfig.txt"
}

wait_builder() {
  phase=$1
  limit=$2
  tick=0
  while [ "$tick" -lt "$limit" ]; do
    status=$(docker inspect -f '{{.State.Status}}' oo-a400-proof 2>/dev/null || printf '%s' missing)
    if [ "$status" != "running" ]; then
      code=$(docker inspect -f '{{.State.ExitCode}}' oo-a400-proof 2>/dev/null || printf '%s' 1)
      printf '%s\n' "$code" > "$RAW/builder-${phase}.exit"
      docker logs oo-a400-proof > "$RAW/builder-${phase}.docker.log" 2>&1 || true
      docker rm oo-a400-proof >/dev/null 2>&1 || true
      return 0
    fi
    sleep 20
    tick=$((tick + 1))
  done
  docker logs oo-a400-proof > "$RAW/builder-${phase}.docker.log" 2>&1 || true
  docker stop oo-a400-proof >/dev/null 2>&1 || true
  docker rm oo-a400-proof >/dev/null 2>&1 || true
  printf '%s\n' 124 > "$RAW/builder-${phase}.exit"
}

log "resume_write_builder"
start_builder write
wait_builder write 120
write_code=$(cat "$RAW/builder-write.exit")
if [ "$write_code" != 0 ]; then
  printf '%s\n' "$write_code" > "$RAW/launch.exit"
  log "resume_write_failed=$write_code"
  exit "$write_code"
fi
if ! grep -q '^live_secret=write_done$' "$STATE/phase.txt"; then
  printf '%s\n' 1 > "$RAW/launch.exit"
  log "resume_write_marker_missing"
  exit 1
fi

log "pg_restart"
docker restart oo-a400-pg >/dev/null
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres oo-a400-pg pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  printf '%s\n' 1 > "$RAW/launch.exit"
  log "pg_restart_not_ready"
  exit 1
fi
printf '%s\n' restarted > "$RAW/pg-restart.txt"

own_rust=$(docker ps -q --filter label=oo.owner=a400 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_rust=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a400 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_rust=%s global_rust=%s host_cargo=%s own_pg=%s\n' "$own_rust" "$global_rust" "$host_cargo" "$own_pg" > "$RAW/capacity-restart.txt"
if [ "$own_rust" -ge 1 ] || [ "$global_rust" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ne 1 ]; then
  echo "deferred: restart capacity own_rust=$own_rust global_rust=$global_rust host_cargo=$host_cargo own_pg=$own_pg" >&2
  printf '%s\n' 3 > "$RAW/launch.exit"
  exit 3
fi

log "restart_builder"
start_builder restart
wait_builder restart 60
restart_code=$(cat "$RAW/builder-restart.exit")
if [ "$restart_code" != 0 ]; then
  printf '%s\n' "$restart_code" > "$RAW/launch.exit"
  log "restart_failed=$restart_code"
  exit "$restart_code"
fi
if ! grep -q '^live_secret=done$' "$STATE/phase.txt"; then
  printf '%s\n' 1 > "$RAW/launch.exit"
  log "restart_marker_missing"
  exit 1
fi

cp "$STATE/run.json" "$RAW/run.json"
cp "$STATE/phase.txt" "$RAW/phase.txt"
if [ -f "$STATE/witness" ]; then
  leaked=0
  for file in "$RAW"/*.log "$RAW"/*.json "$RAW"/*.txt "$RAW"/builder-*.docker.log; do
    if [ -f "$file" ] && grep -F -q -f "$STATE/witness" "$file"; then
      leaked=1
      printf '%s\n' "$file" >> "$RAW/leak-files.txt"
    fi
  done
  if [ "$leaked" -ne 0 ]; then
    printf '%s\n' 1 > "$RAW/launch.exit"
    log "witness_leaked"
    exit 1
  fi
  size=$(wc -c < "$STATE/witness" | tr -d ' ')
  dd if=/dev/zero of="$STATE/witness" bs=1 count="$size" status=none || true
  rm -f "$STATE/witness"
  printf '%s\n' 'witness_absent=true' > "$RAW/witness-disposition.txt"
fi

printf '%s\n' 0 > "$RAW/launch.exit"
log "resume_ok"
echo "resume_ok"
