#!/bin/bash
# One owned Linux arm64 Rust 1.94 builder and one PostgreSQL 17.10 fixture.
# No pull, prune, host network, socket, or foreign cleanup.
set -eu
RUN_ID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
COMMIT=e84efd436dbc105b6a80df62464e1af7534e5af7
ROOT=/tmp/oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a104/SP04/segment-preflight/linux/fixed
REPO=/Users/redux/puppy-one/open-ontology
NAME=oo-a104-msrv-${RUN_ID}
PG=oo-a104-pg-${RUN_ID}
NET=oo-a104-${RUN_ID}-net
CARGO_VOL=oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104-cargo
TARGET_VOL=oo-a104-eef831aa-dba4-4af2-a78b-541bc9e0a104-target
PG_VOL=oo-a104-pg-${RUN_ID}
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a104

mkdir -p "$EVIDENCE" "$ROOT"
chmod +x "$EVIDENCE/inside.sh"
printf '%s\n' "$NAME" > "$EVIDENCE/container-name.txt"
printf '%s\n' "$PG" > "$EVIDENCE/postgres-container.txt"
printf '%s\n' "$NET" > "$EVIDENCE/network-name.txt"
printf '%s\n' "$RUN_ID" > "$EVIDENCE/run-id.txt"

cleanup() {
  status=$?
  set +e
  docker rm "$NAME" > "$EVIDENCE/cleanup-rm.txt" 2>&1
  docker rm -f "$PG" > "$EVIDENCE/cleanup-pg.txt" 2>&1
  docker network rm "$NET" > "$EVIDENCE/cleanup-network.txt" 2>&1
  docker volume rm "$PG_VOL" > "$EVIDENCE/cleanup-pg-volume.txt" 2>&1
  exit "$status"
}
trap cleanup EXIT

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host_toolchain_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/ {print}' || true
  echo "rust_builder_labels:"
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}} {{.Status}} {{.Label "oo.owner"}}' || true
  echo "postgres_labels:"
  docker ps --filter label=oo.kind=postgres --format '{{.Names}} {{.Status}} {{.Label "oo.owner"}}' || true
} > "$EVIDENCE/builders-before.txt"

running=$(docker ps --filter label=oo.project=open-ontology --filter label=oo.kind=rust-builder --format '{{.Names}}')
count=$(printf '%s\n' "$running" | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -ax -o comm= | awk '$1 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/' | wc -l | tr -d ' ')
own_pg=$(docker ps --filter label=oo.owner=a104 --filter label=oo.kind=postgres --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
printf 'builder_containers=%s host_cargo=%s own_postgres=%s\n' "$count" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
if [ "$((count + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builders already at global max" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a104 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a postgres fixture" >&2
  exit 3
fi
docker image inspect "$RUST_IMAGE" >/dev/null
docker image inspect "$PG_IMAGE" >/dev/null

rm -rf "$ROOT/fixed-snapshot" "$ROOT/fixed-archive.tar"
mkdir -p "$ROOT/fixed-snapshot"
git -C "$REPO" archive --format=tar "$COMMIT" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  contracts/pipeline/target-fence-receipts \
  > "$ROOT/fixed-archive.tar"
tar -x -f "$ROOT/fixed-archive.tar" -C "$ROOT/fixed-snapshot"
copy_over() {
  install -m 0644 "$REPO/$1" "$ROOT/fixed-snapshot/$1"
}
copy_over crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql
copy_over crates/oo-pipeline/migrations/README.md
copy_over crates/oo-pipeline/tests/segment_preflight_correction.rs
copy_over contracts/pipeline/target-fence-receipts/target-fence-receipts-v3.json
copy_over contracts/pipeline/target-fence-receipts/README.md
shasum -a 256 "$ROOT/fixed-archive.tar" | awk '{print $1}' > "$EVIDENCE/archive.tar.sha256"
git -C "$REPO" rev-parse "$COMMIT" > "$EVIDENCE/source-commit.txt"
git -C "$REPO" rev-parse "$COMMIT^{tree}" > "$EVIDENCE/source-tree.txt"
git -C "$REPO" rev-parse "$COMMIT^" > "$EVIDENCE/source-parent.txt"
git -C "$REPO" rev-parse HEAD > "$EVIDENCE/workspace-head-at-archive.txt"

python3 - << PY
import hashlib, pathlib
root = pathlib.Path("$ROOT/fixed-snapshot")
repo = pathlib.Path("$REPO")
expect = {
    "crates/oo-pipeline/migrations/0000_bootstrap_roles.sql": "37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208",
    "crates/oo-pipeline/migrations/0001_target_fence_expand.sql": "97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9",
    "crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql": "59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512",
    "contracts/pipeline/target-fence-receipts/target-fence-receipts-v1.json": "429faa9d32e4b42960e805e0883a2f67c8783d936e9094f0747e9c5e21a23521",
    "contracts/pipeline/target-fence-receipts/target-fence-receipts-v2.json": "3a6ca1f4db777f98e8b76f0bd6cc6b5039f2e82d72c478ef4c8fe37713254bac",
}
overlay = [
    "crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql",
    "crates/oo-pipeline/migrations/README.md",
    "crates/oo-pipeline/tests/segment_preflight_correction.rs",
    "contracts/pipeline/target-fence-receipts/target-fence-receipts-v3.json",
    "contracts/pipeline/target-fence-receipts/README.md",
]
for rel, digest in expect.items():
    got = hashlib.sha256((root / rel).read_bytes()).hexdigest()
    if got != digest:
        raise SystemExit(f"preserved hash mismatch {rel} {got}")
for rel in overlay:
    if (root / rel).read_bytes() != (repo / rel).read_bytes():
        raise SystemExit(f"overlay mismatch {rel}")
print("overlay_verified=5 preserved_verified=5")
PY

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a104 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=fixture \
  "$NET"
if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a104 \
    --label oo.session="${SESSION}" \
    --label oo.run_id="${RUN_ID}" \
    --label oo.kind=rust-builder \
    "$CARGO_VOL" >/dev/null
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a104 \
    --label oo.session="${SESSION}" \
    --label oo.run_id="${RUN_ID}" \
    --label oo.kind=rust-builder \
    "$TARGET_VOL" >/dev/null
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a104 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  "$PG_VOL" >/dev/null

docker run -d \
  --name "$PG" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER --cap-add SETGID --cap-add SETUID \
  --memory 1g --cpus 1 --pids-limit 256 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,nodev,size=256m \
  --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  --label oo.project=open-ontology \
  --label oo.owner=a104 \
  --label oo.session="${SESSION}" \
  --label oo.task=segment-preflight-fixed \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp04-segment-preflight \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  "$PG_IMAGE" \
  -c listen_addresses='*' -c max_connections=40 -c shared_preload_libraries= \
  > "$EVIDENCE/postgres-start.txt"

ready=0
for _ in $(seq 1 30); do
  if docker exec "$PG" pg_isready -U postgres -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  exit 1
fi
printf 'ready=1\n' > "$EVIDENCE/postgres-ready.txt"
rm -rf "$EVIDENCE/cases" "$EVIDENCE/summary.log" "$EVIDENCE/live-console.log"

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
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e OO_SEGMENT_PREFLIGHT_HOST="$PG" \
  -e OO_SEGMENT_PREFLIGHT_PORT=5432 \
  -e OO_SEGMENT_PREFLIGHT_EVIDENCE=/evidence \
  --mount "type=bind,source=${ROOT}/fixed-snapshot,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --label oo.project=open-ontology \
  --label oo.owner=a104 \
  --label oo.session="${SESSION}" \
  --label oo.task=segment-preflight-fixed \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp04-segment-preflight \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt" || true
docker inspect "$PG" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}}' > "$EVIDENCE/postgres-inspect.txt" || true
printf 'retained %s\nretained %s\n' "$CARGO_VOL" "$TARGET_VOL" > "$EVIDENCE/cleanup-cargo-volumes.txt"
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
