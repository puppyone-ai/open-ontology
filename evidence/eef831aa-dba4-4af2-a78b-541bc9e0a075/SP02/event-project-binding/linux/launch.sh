#!/bin/bash
# Implementation check. The 70a4e72 baseline is already recorded. This run
# archives HEAD, then overlays only the uncommitted event-binding files.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a075
SOURCE=HEAD
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a075-${RUN_ID}
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/${RUN_ID}/SP02/event-project-binding/linux
NAME=oo-a075-rust194-impl
NET=oo-a075-baseline-net
CARGO_VOL=oo-a075-baseline-cargo
TARGET_VOL=oo-a075-impl-target
BOOT=445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52
EXPAND=b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75
EVENT=4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee
V1=d352c86dbf2836360e541a3511af23674ce9692e687a6ac7041d99118dc8c2ca

mkdir -p "$EVIDENCE"
cp "$SRC/evidence/${RUN_ID}/SP02/event-project-binding/pre-smoke/inside.sh" "$EVIDENCE/inside.sh"
running=$(docker ps --filter label=oo.project=open-ontology --filter label=oo.kind=rust-builder --format '{{.Names}}')
count=$(printf '%s\n' "$running" | sed '/^$/d' | wc -l | tr -d ' ')
printf '%s\n' "$running" > "$EVIDENCE/builders-before.txt"
if [ "$count" -ge 2 ]; then
  echo "deferred: two open-ontology rust builders are already running" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a075 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  printf '%s\n' "$own" > "$EVIDENCE/deferred-own.txt"
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi

rm -rf "$ROOT/snapshot"
mkdir -p "$ROOT/snapshot"
git -C "$SRC" archive --format=tar "$SOURCE" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo contracts \
  | tar -C "$ROOT/snapshot" -xf -
cp "$SRC/crates/oo-jobs/migrations/0002_event_project_binding_expand.sql" \
  "$ROOT/snapshot/crates/oo-jobs/migrations/0002_event_project_binding_expand.sql"
cp "$SRC/crates/oo-jobs/tests/event_project_binding_contract.rs" \
  "$ROOT/snapshot/crates/oo-jobs/tests/event_project_binding_contract.rs"
cp "$SRC/crates/oo-jobs/tests/event_project_fixture.sh" \
  "$ROOT/snapshot/crates/oo-jobs/tests/event_project_fixture.sh"
mkdir -p "$ROOT/snapshot/crates/oo-jobs/tests/fixtures"
cp -R "$SRC/crates/oo-jobs/tests/fixtures/event_project" \
  "$ROOT/snapshot/crates/oo-jobs/tests/fixtures/event_project"
mkdir -p "$ROOT/snapshot/contracts/jobs"
cp -R "$SRC/contracts/jobs/event-project-binding" \
  "$ROOT/snapshot/contracts/jobs/event-project-binding"
cp "$EVIDENCE/inside.sh" "$ROOT/inside.sh"

git -C "$SRC" rev-parse "$SOURCE" > "$EVIDENCE/overlay-head.txt"
git -C "$SRC" rev-parse "${SOURCE}^{tree}" > "$EVIDENCE/overlay-tree.txt"
git -C "$SRC" rev-parse "${SOURCE}^" > "$EVIDENCE/overlay-parent.txt"

hash_pair() {
  local file=$1
  local expect=$2
  local shasum_hex python_hex
  shasum_hex=$(shasum -a 256 "$file" | awk '{print $1}')
  python_hex=$(python3 -c 'import hashlib, pathlib, sys; print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())' "$file")
  printf '%s shasum=%s python=%s\n' "$file" "$shasum_hex" "$python_hex"
  if [ "$shasum_hex" != "$python_hex" ] || [ "$shasum_hex" != "$expect" ]; then
    echo "hash mismatch for $file" >&2
    exit 1
  fi
}

{
  hash_pair "$ROOT/snapshot/crates/oo-jobs/migrations/0000_bootstrap_roles.sql" "$BOOT"
  hash_pair "$ROOT/snapshot/crates/oo-jobs/migrations/0001_jobs_control_expand.sql" "$EXPAND"
  hash_pair "$ROOT/snapshot/crates/oo-jobs/migrations/0002_event_project_binding_expand.sql" "$EVENT"
  hash_pair "$ROOT/snapshot/contracts/jobs/control-schema/jobs-control-v1.json" "$V1"
  hash_pair "$SRC/crates/oo-jobs/migrations/0000_bootstrap_roles.sql" "$BOOT"
  hash_pair "$SRC/crates/oo-jobs/migrations/0001_jobs_control_expand.sql" "$EXPAND"
  hash_pair "$SRC/crates/oo-jobs/migrations/0002_event_project_binding_expand.sql" "$EVENT"
  hash_pair "$SRC/contracts/jobs/control-schema/jobs-control-v1.json" "$V1"
} | tee "$EVIDENCE/hashes.txt"

find "$ROOT/snapshot" -type f | sort | shasum -a 256 > "$EVIDENCE/snapshot-files.sha256"

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
kind=implementation overlay
overlay_head=$(cat "$EVIDENCE/overlay-head.txt")
overlay_parent=$(cat "$EVIDENCE/overlay-parent.txt")
overlay_tree=$(cat "$EVIDENCE/overlay-tree.txt")
baseline_commit=70a4e72c3ea698038e0bdd38df29e38c01dfd24d
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
snapshot_method=git archive HEAD of Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo contracts, then overlay uncommitted event-binding files
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
cap_drop=ALL
cap_add=none
init=yes
no_new_privileges=yes
airgap_claim=no
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-jobs; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-jobs --all-targets -- -D warnings
EOF

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a075 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$NET" >/dev/null 2>&1 || true
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a075 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null || true
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a075 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null || true

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
  --mount "type=bind,source=${ROOT}/snapshot,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a075 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=event-project-impl \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp02-event-project-binding \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}} Privileged={{.HostConfig.Privileged}}' > "$EVIDENCE/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "$EVIDENCE/cap-drop.json"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/ports.json"
docker cp "$NAME":/target/phases "$EVIDENCE/phases" || true
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
