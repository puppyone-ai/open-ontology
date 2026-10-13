#!/bin/bash
# DL01-W1-41 native MongoDB driver gate. Owned containers only.
set -euo pipefail

ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/mongodb"
EVIDENCE="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-driver-preflight"
SECRETS="/tmp/oo-dl01w1-41-secrets"
PREFIX="oo-dl01w1-41"
MONGO="${PREFIX}-mongodb"
RUST="${PREFIX}-rust"
PROBE="${PREFIX}-probe"
RUST_IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
MONGO_IMAGE="mongo@sha256:d0d926f94df099bff534b7ee5b5986458131a22489dfff8664509af0c1e2ca9c"
ADMIT="$SRC/scripts/admit.py"
BUILD="$EVIDENCE/raw/build"
FIX="$EVIDENCE/raw/fixture"
PROBE_DIR="$EVIDENCE/raw/probe"
AUDIT="$EVIDENCE/raw/audit"
DBA_URI="mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-dba&maxPoolSize=1"
AUDIT_URI="mongodb://127.0.0.1:27017/admin?appName=oo-dl01w1-41-audit&maxPoolSize=1"
LABELS=(
  --label project=open-ontology
  --label owner=dl01-w1-41
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd
  --label task=DL01-W1-41
)
final_ok=0
cleanup_done=0

phase() {
  mkdir -p "$EVIDENCE/raw"
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" | tee -a "$EVIDENCE/raw/phase.txt"
}

rand_pw() {
  printf 'Aa1%s' "$(openssl rand -hex 12)"
}

redact_stream() {
  python3 - "$SECRETS" <<'PY'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
secrets = []
if root.is_dir():
    for path in root.iterdir():
        if path.is_file():
            text = path.read_text(errors="replace").strip()
            if len(text) >= 12:
                secrets.append(text)
data = sys.stdin.buffer.read().decode("utf-8", "replace")
for secret in secrets:
    data = data.replace(secret, "[redacted]")
sys.stdout.write(data)
PY
}

remove_owned_containers() {
  local id owner
  for id in $(docker ps -aq --filter "name=${PREFIX}"); do
    owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' "$id" 2>/dev/null || true)"
    if [[ "$owner" == "dl01-w1-41" ]]; then
      docker rm -f "$id" >/dev/null
    fi
  done
}

admit_wait() {
  local kind="$1" dest="$2" attempt
  mkdir -p "$(dirname "$dest")"
  for attempt in $(seq 1 30); do
    if python3 "$ADMIT" acquire "$kind" "$dest"; then
      return 0
    fi
    phase "capacity wait ${kind} attempt ${attempt}"
    sleep 900
  done
  phase "capacity denied ${kind}"
  return 3
}

reclaimable_bytes() {
  python3 - <<'PY'
import subprocess
page = int(subprocess.check_output(["pagesize"], text=True).strip())
text = subprocess.check_output(["vm_stat"], text=True)
keys = ["Pages free", "Pages inactive", "Pages speculative", "Pages purgeable"]
total = 0
for line in text.splitlines():
    for key in keys:
        if line.startswith(key):
            total += int(line.split(":", 1)[1].strip().strip("."))
print(total * page)
PY
}

wait_combined_memory() {
  local bytes need floor attempt
  need=$((8 * 1024 * 1024 * 1024))
  floor=$((10 * 1024 * 1024 * 1024))
  for attempt in $(seq 1 30); do
    bytes="$(reclaimable_bytes)"
    if [[ $((bytes - need)) -ge $floor ]]; then
      phase "combined memory ok reclaimable=${bytes}"
      return 0
    fi
    phase "combined memory wait ${attempt} reclaimable=${bytes}"
    sleep 900
  done
  return 3
}

reserved_cpus() {
  python3 - <<'PY'
import subprocess
proc = subprocess.run(["docker", "ps", "-q"], text=True, capture_output=True)
ids = [line for line in proc.stdout.splitlines() if line]
if not ids:
    print("0")
    raise SystemExit
inspected = subprocess.run(
    ["docker", "inspect", "-f", "{{.HostConfig.NanoCpus}}", *ids],
    text=True,
    capture_output=True,
)
total = 0
for line in inspected.stdout.splitlines():
    try:
        total += int(line.strip() or "0")
    except ValueError:
        continue
print(total / 1_000_000_000)
PY
}

wait_pair_capacity() {
  local attempt cpus bytes need floor
  need=$((8 * 1024 * 1024 * 1024))
  floor=$((10 * 1024 * 1024 * 1024))
  for attempt in $(seq 1 30); do
    cpus="$(reserved_cpus)"
    bytes="$(reclaimable_bytes)"
    if python3 -c 'import sys; cpus=float(sys.argv[1]); mem=int(sys.argv[2]); need=int(sys.argv[3]); floor=int(sys.argv[4]); raise SystemExit(0 if cpus + 2 <= 16 and mem - need >= floor else 1)' "$cpus" "$bytes" "$need" "$floor"; then
      phase "pair capacity ok cpus=${cpus} reclaimable=${bytes}"
      return 0
    fi
    phase "pair capacity wait ${attempt} cpus=${cpus} reclaimable=${bytes}"
    sleep 900
  done
  phase "pair capacity denied"
  return 3
}

preserve_previous() {
  if [[ ! -f "$BUILD/docker.exit" && ! -f "$PROBE_DIR/probe.exit" ]]; then
    return 0
  fi
  local n=1 dest
  while [[ -e "$EVIDENCE/raw/attempts/$(printf '%02d' "$n")" ]]; do
    n=$((n + 1))
  done
  dest="$EVIDENCE/raw/attempts/$(printf '%02d' "$n")"
  mkdir -p "$dest"
  for part in build probe fixture audit; do
    if [[ -d "$EVIDENCE/raw/$part" ]]; then
      mv "$EVIDENCE/raw/$part" "$dest/$part"
    fi
  done
  phase "preserved previous attempt ${dest}"
}

mongosh_file() {
  local name="$1" uri="$2" script="$3" dest="$4"
  mkdir -p "$dest"
  docker cp "$script" "$MONGO:/tmp/oo-${name}.js" >"$dest/${name}-copy.out" 2>"$dest/${name}-copy.err"
  set +e
  docker exec -u 0 -e HOME=/tmp "$MONGO" \
    mongosh --quiet --norc "$uri" --file "/tmp/oo-${name}.js" \
    >"$dest/${name}.out" 2>"$dest/${name}.err"
  local rc=$?
  printf '%s\n' "$rc" >"$dest/${name}.exit"
  return 0
}

cleanup() {
  if [[ "$cleanup_done" == "1" ]]; then
    return
  fi
  cleanup_done=1
  set +e
  if docker inspect "$MONGO" >/dev/null 2>&1; then
    docker logs "$MONGO" 2>&1 | redact_stream >"$FIX/docker-logs.txt" || true
    docker inspect "$MONGO" | redact_stream >"$FIX/docker-inspect.json" || true
  fi
  if docker inspect "$PROBE" >/dev/null 2>&1; then
    docker inspect "$PROBE" | redact_stream >"$PROBE_DIR/probe-inspect.json" || true
  fi
  remove_owned_containers
  docker volume rm "${PREFIX}-data" >/dev/null 2>&1 || true
  if [[ "$final_ok" == "1" ]]; then
    docker volume rm "${PREFIX}-cargo" "${PREFIX}-target" >/dev/null 2>&1 || true
  fi
  docker network rm "${PREFIX}-net" >/dev/null 2>&1 || true
  rm -rf "$SECRETS"
  python3 "$ADMIT" release all >/dev/null 2>&1 || true
  python3 - "$EVIDENCE" "$final_ok" "$RUST_IMAGE" "$MONGO_IMAGE" <<'PY'
import json, pathlib, sys
evidence, final_ok, rust_image, mongo_image = sys.argv[1:]
path = pathlib.Path(evidence) / "raw" / "resource-receipt.json"
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps({
    "task_id": "DL01-W1-41",
    "prefix": "oo-dl01w1-41",
    "promoted": final_ok == "1",
    "kept_cargo_and_target_volumes": final_ok != "1",
    "images_retained": [rust_image, mongo_image],
    "secrets_removed": True,
    "foreign_containers_untouched": True,
}, indent=2) + "\n")
PY
  phase "cleanup final_ok=${final_ok}"
}

trap cleanup EXIT

mkdir -p "$EVIDENCE/raw" "$BUILD" "$FIX" "$PROBE_DIR" "$AUDIT"
phase "start"
python3 "$SRC/scripts/oracle.py" --self-check
python3 "$ADMIT" release all >/dev/null
remove_owned_containers
preserve_previous
mkdir -p "$BUILD" "$FIX" "$PROBE_DIR" "$AUDIT"

docker image inspect "$RUST_IMAGE" >/dev/null
docker image inspect "$MONGO_IMAGE" >/dev/null
docker image inspect "$MONGO_IMAGE" --format '{{json .RepoDigests}} {{.Id}}' >"$FIX/mongo-image.txt"
docker image inspect "$RUST_IMAGE" --format '{{.Id}}' >"$BUILD/rust-image.txt"
if ! docker network inspect "${PREFIX}-net" >/dev/null 2>&1; then
  docker network create \
    --label project=open-ontology --label owner=dl01-w1-41 \
    --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
    --label task=DL01-W1-41 --label kind=mongodb-driver-preflight \
    "${PREFIX}-net" >/dev/null
fi
if ! docker volume inspect "${PREFIX}-cargo" >/dev/null 2>&1; then
  docker volume create "${PREFIX}-cargo" \
    --label project=open-ontology --label owner=dl01-w1-41 \
    --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
    --label task=DL01-W1-41 --label kind=rust-builder >/dev/null
fi
if ! docker volume inspect "${PREFIX}-target" >/dev/null 2>&1; then
  docker volume create "${PREFIX}-target" \
    --label project=open-ontology --label owner=dl01-w1-41 \
    --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
    --label task=DL01-W1-41 --label kind=rust-builder >/dev/null
fi

rm -rf "$SECRETS"
mkdir -m 0700 "$SECRETS"
umask 077
printf '%s\n' "$(rand_pw)" >"$SECRETS/dba"
printf '%s\n' "$(rand_pw)" >"$SECRETS/reader"
printf '%s\n' "$(rand_pw)" >"$SECRETS/audit"
chmod 600 "$SECRETS/dba" "$SECRETS/reader" "$SECRETS/audit"

phase "build"
admit_wait rust "$BUILD/admission.json"
set +e
docker run --name "$RUST" \
  --platform linux/arm64 --pull never \
  --network "${PREFIX}-net" \
  --memory 6g --memory-swap 6g --cpus 1 \
  --pids-limit 1024 \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_INCREMENTAL=1 \
  -v "${PREFIX}-cargo:/cargo" \
  -v "${PREFIX}-target:/target" \
  -v "$SRC:/src" \
  -v "$BUILD:/evidence" \
  "${LABELS[@]}" \
  --label kind=rust-builder \
  "$RUST_IMAGE" \
  bash /src/scripts/build-inside.sh >"$BUILD/docker.stdout" 2>"$BUILD/docker.stderr"
build_rc=$?
set -e
printf '%s\n' "$build_rc" >"$BUILD/docker.exit"
docker rm -f "$RUST" >/dev/null 2>&1 || true
python3 "$ADMIT" release rust >/dev/null
if [[ "$build_rc" -ne 0 ]]; then
  phase "build failed ${build_rc}"
  exit "$build_rc"
fi
python3 "$SRC/scripts/check-lock.py" "$SRC/Cargo.lock" "$BUILD/lock-checksums-host.txt" "$BUILD/Cargo.lock.before"
shasum -a 256 "$BUILD/bin/oo-mongodb-driver-probe" "$BUILD/bin/oo-mongodb-arrow-decode" >"$BUILD/bin-sha256.txt"
phase "build ok"

phase "pair-capacity"
wait_pair_capacity

phase "mongodb"
docker volume rm "${PREFIX}-data" >/dev/null 2>&1 || true
docker volume create "${PREFIX}-data" \
  --label project=open-ontology --label owner=dl01-w1-41 \
  --label session=23df6145-4592-4dc0-82df-fd226dd020fd \
  --label task=DL01-W1-41 --label kind=mongodb-source >/dev/null
admit_wait mongodb "$FIX/admission.json"
printf '%s\n' 'mongod --bind_ip 127.0.0.1 --port 27017 --auth --wiredTigerCacheSizeGB 0.25 --setParameter diagnosticDataCollectionEnabled=false' >"$FIX/mongod-argv.txt"
docker run -d --name "$MONGO" \
  --platform linux/arm64 --pull never \
  --network "${PREFIX}-net" \
  --memory 2g --memory-swap 2g --cpus 1 \
  --pids-limit 512 \
  -v "${PREFIX}-data:/data/db" \
  -v "$SECRETS:/run/secrets:ro" \
  "${LABELS[@]}" \
  --label kind=mongodb-source \
  "$MONGO_IMAGE" \
  mongod --bind_ip 127.0.0.1 --port 27017 --auth \
  --wiredTigerCacheSizeGB 0.25 \
  --setParameter diagnosticDataCollectionEnabled=false \
  >"$FIX/mongo-run.out"
ready=0
for attempt in $(seq 1 15); do
  sleep 2
  set +e
  docker exec -u 0 -e HOME=/tmp "$MONGO" \
    mongosh --quiet --norc "$DBA_URI" \
    --eval 'const r = db.runCommand({ ping: 1 }); if (r.ok === 1) quit(0); quit(2);' \
    >"$FIX/ping.out" 2>"$FIX/ping.err"
  ping_rc=$?
  set -e
  if [[ "$ping_rc" -eq 0 ]]; then
    ready=1
    break
  fi
done
printf '%s\n' "$ready" >"$FIX/ping.ready"
if [[ "$ready" -ne 1 ]]; then
  phase "mongodb ping failed"
  exit 4
fi
phase "fixture"
set +e
mongosh_file dba-setup "$DBA_URI" "$SRC/scripts/dba-setup.js" "$FIX"
set -e
docker cp "$MONGO:/tmp/oo-dl01w1-41-out/." "$FIX/" || true
phase "extract"
set +e
mongosh_file extract "$DBA_URI" "$SRC/scripts/extract.js" "$FIX"
set -e
docker cp "$MONGO:/tmp/oo-dl01w1-41-out/." "$FIX/" || true
if [[ "$(cat "$FIX/dba-setup.exit" 2>/dev/null || echo 9)" != "0" || "$(cat "$FIX/extract.exit" 2>/dev/null || echo 9)" != "0" ]]; then
  phase "fixture failed"
  exit 4
fi

phase "probe"
wait_combined_memory
rm -rf "$PROBE_DIR"
mkdir -p "$PROBE_DIR"
admit_wait rust "$PROBE_DIR/admission.json"
set +e
docker run --name "$PROBE" \
  --platform linux/arm64 --pull never \
  --network "container:${MONGO}" \
  --memory 6g --memory-swap 6g --cpus 1 \
  --pids-limit 512 \
  --entrypoint bash \
  -e OO_OUT=/out \
  -e OO_READER_PASSWORD_FILE=/run/secrets/reader \
  -e OO_W141_CUSTOMER=/run/customer.json \
  -e OO_W141_SECRET_REGISTRY=/run/secret-registry.json \
  -v "$PROBE_DIR:/out" \
  -v "$SECRETS/reader:/run/secrets/reader:ro" \
  -v "$SRC/scripts/customer.preflight.json:/run/customer.json:ro" \
  -v "$ROOT/tools/mongodb-preview/examples/secret-registry.json:/run/secret-registry.json:ro" \
  -v "$BUILD/bin:/binaries:ro" \
  -v "$SRC/scripts/probe-inside.sh:/probe-inside.sh:ro" \
  "${LABELS[@]}" \
  --label kind=rust-builder \
  "$RUST_IMAGE" \
  /probe-inside.sh >"$PROBE_DIR/docker.stdout" 2>"$PROBE_DIR/docker.stderr"
probe_rc=$?
set -e
printf '%s\n' "$probe_rc" >"$PROBE_DIR/docker.exit"
docker inspect "$PROBE" >"$PROBE_DIR/probe-inspect.json" 2>/dev/null || true
docker rm -f "$PROBE" >/dev/null 2>&1 || true
python3 "$ADMIT" release rust >/dev/null
phase "probe container ${probe_rc}"

phase "audit"
set +e
mongosh_file audit "$AUDIT_URI" "$SRC/scripts/audit.js" "$AUDIT"
set -e
docker cp "$MONGO:/tmp/oo-dl01w1-41-audit/." "$AUDIT/" || true
phase "oracle"
set +e
python3 "$SRC/scripts/oracle.py" \
  --ejson "$FIX/canonical-ejson.json" \
  --decoded "$PROBE_DIR/decoded.json" \
  --decoded-truncated "$PROBE_DIR/decoded-truncated.json" \
  --decode-exit "$PROBE_DIR/decode.exit" \
  --decode-truncated-exit "$PROBE_DIR/decode-truncated.exit" \
  --gates "$PROBE_DIR/gates.json" \
  --commands "$PROBE_DIR/commands.json" \
  --connections "$PROBE_DIR/connections.json" \
  --wire "$PROBE_DIR/opmsg-names.jsonl" \
  --pre-read "$PROBE_DIR/pre-read-deny.json" \
  --catalog "$PROBE_DIR/catalog.json" \
  --census "$PROBE_DIR/field-census.json" \
  --rejections "$PROBE_DIR/rejections.json" \
  --timeout "$PROBE_DIR/timeout.json" \
  --change "$PROBE_DIR/change-stream.json" \
  --cancel "$PROBE_DIR/cancel.json" \
  --truncation "$PROBE_DIR/truncation.json" \
  --options "$PROBE_DIR/client-options.json" \
  --profile-admin "$AUDIT/profile-admin.json" \
  --profile-db "$AUDIT/profile-oo_w141.json" \
  --audit "$AUDIT/audit-result.json" \
  --evidence "$EVIDENCE" \
  --source "$SRC" \
  --secrets "$SECRETS" \
  --batch-arrow "$EVIDENCE/batch.arrow" \
  --partial "$PROBE_DIR/batch.arrow.partial" \
  --execution-mode "$PROBE_DIR/execution-mode.txt" \
  --out "$EVIDENCE/raw/oracle-result.json"
oracle_rc=$?
set -e
printf '%s\n' "$oracle_rc" >"$EVIDENCE/raw/oracle.exit"
if [[ "$oracle_rc" -eq 0 ]]; then
  python3 - "$PROBE_DIR/batch.arrow.partial" "$EVIDENCE/batch.arrow" "$EVIDENCE/raw/oracle-result.json" <<'PY'
import json, pathlib, sys
partial, ready, result = map(pathlib.Path, sys.argv[1:])
body = json.loads(result.read_text())
if body.get("ready") is not True or not partial.is_file() or ready.exists():
    raise SystemExit(1)
partial.replace(ready)
print("promoted")
PY
  printf '%s\n' true >"$EVIDENCE/raw/promotion.txt"
  final_ok=1
  phase "promoted"
else
  printf '%s\n' false >"$EVIDENCE/raw/promotion.txt"
  phase "oracle failed"
  exit 1
fi
phase "gate commands finished"
