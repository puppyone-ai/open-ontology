#!/bin/bash
# Private PG 17.10 plus one Rust 1.94 probe. Archive plus overlay. No host cargo.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a156/SP07/captured-pg-draft
ROOT=/tmp/oo-a156-eef831aa
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a156-probe
CARGO_VOL=oo-a156-eef831aa-cargo
TARGET_VOL=oo-a156-eef831aa-target
NET=oo-a156-eef831aa-net
FIXED=e6bd4c7aa8c3a43cc0516a21e325765c5eecb2f6
SNAP=${ROOT}/probe-snapshot
OUT=${EVIDENCE}/probe/out
SEC=/tmp/oo-a156-eef831aa-dba4-4af2-a78b-541bc9e0a156/secrets

mkdir -p "$OUT" "$ROOT"
chmod 755 "$EVIDENCE/probe/inside.sh" "$EVIDENCE/probe/launch.sh" "$EVIDENCE/pg/up.sh" "$EVIDENCE/pg/init.sh" "$EVIDENCE/pg/pg-wrap.sh" "$EVIDENCE/pg/audit.py"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/probe/containers-before.txt" || true
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a156 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a156 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s own_postgres_running=%s\n' "$running_rust" "$own" "$host_cargo" "$own_pg" > "$EVIDENCE/probe/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/probe/launch.exit"
  exit 3
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo \
  | tar -x -C "$SNAP"
overlay() { cp "$SRC/$1" "$SNAP/$1"; }
overlay crates/oo-builder/src/lib.rs
overlay crates/oo-builder/src/draft.rs
overlay crates/oo-builder/src/pgsql.rs
overlay crates/oo-builder/src/captured_pg.rs
overlay crates/oo-builder/tests/captured_pg.rs
mkdir -p "$SNAP/contracts/builder" "$SNAP/probe/src"
cp "$SRC/contracts/builder/captured-pg-draft-v1.md" "$SNAP/contracts/builder/captured-pg-draft-v1.md"
cp "$EVIDENCE/probe/Cargo.toml" "$SNAP/probe/Cargo.toml"
cp "$EVIDENCE/probe/src/main.rs" "$SNAP/probe/src/main.rs"
python3 - "$SNAP/Cargo.toml" <<'PY'
import sys
from pathlib import Path
path = Path(sys.argv[1])
text = path.read_text()
old = '    "crates/xtask",\n]'
new = '    "crates/xtask",\n    "probe",\n]'
if '"probe"' not in text:
    if old not in text:
        raise SystemExit("workspace members anchor missing")
    text = text.replace(old, new, 1)
    path.write_text(text)
PY
test -f "$SNAP/crates/oo-builder/src/captured_pg.rs"
test -f "$SNAP/probe/src/main.rs"
test -f "$SNAP/crates/oo-connectors/vendor/sqlx-core/Cargo.toml"
cp "$SNAP/Cargo.lock" "$OUT/Cargo.lock.before"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/probe/source-tree.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/probe/source-commit.txt"
shasum -a 256 \
  "$SNAP/crates/oo-builder/src/lib.rs" \
  "$SNAP/crates/oo-builder/src/draft.rs" \
  "$SNAP/crates/oo-builder/src/pgsql.rs" \
  "$SNAP/crates/oo-builder/src/captured_pg.rs" \
  "$SNAP/crates/oo-builder/tests/captured_pg.rs" \
  "$SNAP/probe/src/main.rs" \
  > "$EVIDENCE/probe/overlay.sha256"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a156 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a156 \
    "$NET" > "$EVIDENCE/probe/network.txt"
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a156 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a156 --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a156 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a156 --label oo.kind=target \
  "$TARGET_VOL" >/dev/null

if docker ps --filter name='^/oo-a156-pg$' --filter status=running -q | grep -q .; then
  echo "pg_already_running name=oo-a156-pg"
else
  bash "$EVIDENCE/pg/up.sh"
fi

run_probe() {
  phase="$1"
  if docker container inspect "$NAME" >/dev/null 2>&1; then
    prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
    if [ "$prior" = "running" ]; then
      echo "deferred: probe container is already running" >&2
      return 3
    fi
    docker rm "$NAME" >/dev/null
  fi
  set +e
  docker run \
    --name "$NAME" \
    --platform linux/arm64 \
    --pull never \
    --init \
    --user 0:0 \
    --security-opt no-new-privileges \
    --cap-drop ALL \
    --memory 8g \
    --memory-swap 8g \
    --cpus 4 \
    --pids-limit 4096 \
    --network "$NET" \
    --workdir /src \
    -e CARGO_TERM_COLOR=never \
    -e CARGO_HOME=/cargo-home \
    -e CARGO_TARGET_DIR=/target \
    -e CARGO_BUILD_JOBS=4 \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    -e HOME=/tmp \
    -e "PHASE=${phase}" \
    -e PGHOST=oo-a156-pg \
    -e PGPORT=5432 \
    -e PGDATABASE=oo_a156 \
    -e PGUSER=oo_a156_reader \
    -e PASSWORD_FILE=/secrets/reader.pass \
    -e CA_FILE=/secrets/ca.crt \
    -e OUT_DIR=/out \
    --mount "type=bind,source=${SNAP},target=/src" \
    --mount "type=bind,source=${EVIDENCE}/probe/inside.sh,target=/inside.sh,readonly" \
    --mount "type=bind,source=${OUT},target=/out" \
    --mount "type=bind,source=${SEC},target=/secrets,readonly" \
    --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
    --mount "type=volume,source=${TARGET_VOL},target=/target" \
    --label oo.project=open-ontology \
    --label oo.owner=a156 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a156 \
    --label oo.task=captured-pg-draft-probe \
    --label oo.toolchain=1.94.0 \
    --label oo.platform=linux/arm64 \
    --label oo.kind=rust-builder \
    "$RUST_IMAGE" \
    bash /inside.sh > "$OUT/${phase}-console.log" 2>&1
  code=$?
  set -e
  printf '%s\n' "$code" > "$OUT/${phase}-docker.exit"
  {
    echo "phase=${phase}"
    echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
    echo "Status=$(docker inspect -f '{{.State.Status}}' "$NAME")"
    echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
    echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
    echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
    echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
    echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
    echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
    echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  } > "$OUT/${phase}-hostconfig.txt"
  return "$code"
}

set +e
run_probe capture
capture_code=$?
set -e
if [ "$capture_code" -ne 0 ]; then
  printf '%s\n' "$capture_code" > "$EVIDENCE/probe/launch.exit"
  exit "$capture_code"
fi

set +e
docker exec -u postgres oo-a156-pg psql -v ON_ERROR_STOP=1 -d oo_a156 -f /fixture/alter-note.sql > "$OUT/alter.stdout" 2> "$OUT/alter.stderr"
alter_code=$?
set -e
printf '%s\n' "$alter_code" > "$OUT/alter.exit"
if [ "$alter_code" -ne 0 ]; then
  printf '%s\n' "$alter_code" > "$EVIDENCE/probe/launch.exit"
  exit "$alter_code"
fi

set +e
run_probe stale
stale_code=$?
set -e
if [ "$stale_code" -ne 0 ]; then
  printf '%s\n' "$stale_code" > "$EVIDENCE/probe/launch.exit"
  exit "$stale_code"
fi

docker exec oo-a156-pg cat /pglog/postgresql.log > "$ROOT/postgresql.log"
set +e
python3 "$EVIDENCE/pg/audit.py" "$ROOT/postgresql.log" "$EVIDENCE/pg/audit.txt" "$SEC"
audit_code=$?
set -e
rm -f "$ROOT/postgresql.log"
printf '%s\n' "$audit_code" > "$EVIDENCE/pg/audit.exit"
if [ "$audit_code" -ne 0 ]; then
  printf '%s\n' "$audit_code" > "$EVIDENCE/probe/launch.exit"
  exit "$audit_code"
fi

if [ -f "$SNAP/Cargo.lock" ]; then
  cp "$SNAP/Cargo.lock" "$OUT/Cargo.lock.after"
  diff -u "$OUT/Cargo.lock.before" "$OUT/Cargo.lock.after" > "$OUT/Cargo.lock.diff" || true
fi
printf '%s\n' 0 > "$EVIDENCE/probe/launch.exit"
echo "PROBE_LAUNCH_OK"
