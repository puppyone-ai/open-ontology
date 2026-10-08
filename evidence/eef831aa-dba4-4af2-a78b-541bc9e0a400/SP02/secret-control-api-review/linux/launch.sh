#!/bin/bash
# Host launcher. One 6 GiB builder and one PostgreSQL 17.10. No host port, no host network,
# no docker socket, no host home, no shared cargo cache.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a400
FIXED=bb831355662c1dffd76b86bf60417158f4513134
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/SP02/secret-control-api-review
RAW=${ROOT}/linux/raw
PG=${ROOT}/pg
SEC=/tmp/oo-a400-${RUN_ID}/secrets
STATE=/tmp/oo-a400-${RUN_ID}/state
ARCHIVE=/tmp/oo-a400-${RUN_ID}/archive
PROOF=/tmp/oo-a400-${RUN_ID}/proof
NET=oo-a400-eef831aa-net
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
mkdir -p "$RAW"
cd "$REPO"

log() {
  printf '%s\n' "$1" >> "$RAW/launch.log"
}

own_rust=$(docker ps -q --filter label=oo.owner=a400 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_rust=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a400 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_rust=%s global_rust=%s host_cargo=%s own_pg=%s\n' "$own_rust" "$global_rust" "$host_cargo" "$own_pg" > "$RAW/capacity-before.txt"
if [ "$own_rust" -ge 1 ] || [ "$global_rust" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ge 1 ]; then
  echo "deferred: capacity own_rust=$own_rust global_rust=$global_rust host_cargo=$host_cargo own_pg=$own_pg" >&2
  printf '%s\n' 3 > "$RAW/launch.exit"
  exit 3
fi

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a400 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" > "$RAW/network.id"
fi

bash "$PG/up.sh"
docker exec -u root oo-a400-pg /fixture/init-roles.sh > "$RAW/init-roles.txt"
grep -q '^ok$' "$RAW/init-roles.txt"
docker exec -u postgres oo-a400-pg psql -U oo_fixture_admin -d identity -v ON_ERROR_STOP=1 -c 'DROP SCHEMA IF EXISTS oo_identity_control CASCADE' > "$RAW/reset-identity.txt"
docker exec -u postgres oo-a400-pg psql -U oo_fixture_admin -d secret -v ON_ERROR_STOP=1 -c 'DROP SCHEMA IF EXISTS oo_secret_control CASCADE' > "$RAW/reset-secret.txt"

rm -rf "$ARCHIVE" "$PROOF"
mkdir -p "$ARCHIVE"
git -C "$REPO" archive "$FIXED" tools/secret-control-api tools/control-identity | tar -x -C "$ARCHIVE"
mkdir -p "$PROOF"
cp -a "$ARCHIVE/tools/secret-control-api/." "$PROOF/"
mkdir -p "$PROOF/tests/independent"
cp "$ROOT/overlay/independent/main.rs" "$ROOT/overlay/independent/idp.rs" "$PROOF/tests/independent/"
python3 - <<PY
import hashlib, os, pathlib, sys
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
path = pathlib.Path("$RAW/archive-secret.sha256")
path.write_text("\n".join(left) + "\n")
overlay = pathlib.Path("$RAW/overlay-sha256.txt")
lines = []
for name in ("main.rs", "idp.rs"):
    file = proof / "tests/independent" / name
    lines.append(hashlib.sha256(file.read_bytes()).hexdigest() + "  tests/independent/" + name)
overlay.write_text("\n".join(lines) + "\n")
print("archive_match=yes")
print("archive_files=%s" % len(left))
PY
printf '%s\n' "$FIXED" > "$RAW/fixed-commit.txt"
{
  echo "origin_commit=a587031c719a484830d78e493605f48e2056cae4"
  echo "fixed_commit=$FIXED"
  echo "25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/pg-wrap.sh"
  echo "96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh"
  echo "d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3  deploy/trusted-demo/tools/sha256_manifest.rs"
  echo "pg-wrap.sh in this packet is the same bytes as the a200 recipe."
  echo "sha256_manifest.rs in this packet is the same bytes as deploy/trusted-demo/tools/sha256_manifest.rs."
  echo "up.sh keeps that origin image digest, capability set, memory, cpu, private bridge, and secret-file entrypoint. Resource names are a400-private."
} > "$RAW/recipe-origin.txt"

docker volume create --label oo.owner=a400 --label oo.session="${RUN_ID}" --label oo.kind=cargo oo-a400-cargo > "$RAW/volume-cargo.txt"
docker volume create --label oo.owner=a400 --label oo.session="${RUN_ID}" --label oo.kind=work oo-a400-work > "$RAW/volume-work.txt"

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

log "write_builder"
start_builder write
wait_builder write 180
write_code=$(cat "$RAW/builder-write.exit")
if [ "$write_code" != 0 ]; then
  printf '%s\n' "$write_code" > "$RAW/launch.exit"
  log "write_failed=$write_code"
  exit "$write_code"
fi
if ! grep -q '^live_secret=write_done$' "$STATE/phase.txt"; then
  printf '%s\n' 1 > "$RAW/launch.exit"
  log "write_marker_missing"
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
wait_builder restart 90
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
log "launch_ok"
echo "launch_ok"
