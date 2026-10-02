#!/bin/bash
# Replacement baseline. Attempt 1 omitted contracts/, so the jobs contract test
# could not read jobs-control-v1.json. Fetch and clippy on that attempt exited 0.
# This run keeps the same commit and adds contracts to the finite snapshot.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a075
SOURCE=70a4e72c3ea698038e0bdd38df29e38c01dfd24d
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a075-${RUN_ID}
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/${RUN_ID}/SP02/event-project-binding/baseline
NAME=oo-a075-rust194-baseline2
NET=oo-a075-baseline-net
CARGO_VOL=oo-a075-baseline-cargo
TARGET_VOL=oo-a075-baseline-target

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
cp "$EVIDENCE/inside.sh" "$ROOT/inside.sh"
git -C "$SRC" rev-parse "$SOURCE" > "$EVIDENCE/baseline-head.txt"
git -C "$SRC" rev-parse "${SOURCE}^{tree}" > "$EVIDENCE/baseline-tree.txt"
git -C "$SRC" rev-parse "${SOURCE}^" > "$EVIDENCE/baseline-parent.txt"
find "$ROOT/snapshot" -type f | sort | shasum -a 256 > "$EVIDENCE/snapshot-files.sha256"

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
replaces=pre-smoke attempt 1, test exit 101, missing /src/contracts
baseline_head=${SOURCE}
baseline_parent=6790bb641e97098326a6994cbfc9a316c3c11fd9
baseline_tree=0d1c949ae35c68c07ecaf08438a2ea23944eedd3
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
snapshot_method=git archive of the committed tree plus contracts
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
  --label oo.task=event-project-baseline \
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
