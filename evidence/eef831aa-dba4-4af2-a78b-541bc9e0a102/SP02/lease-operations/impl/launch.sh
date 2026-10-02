#!/bin/bash
# Focused Linux arm64 Rust 1.94 smoke of the committed lease-operations tree.
# The ignored live fixture is not part of this command: no docker socket, no psql.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a102
COMMIT=c312cae2daf75cfaa0b618066d709b9e529b3f95
TREE=19319751f04a7cb048bf3980a5408357d10acf58
PARENT=4477f23205e989ac3c2207098efc369e6286cb98
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a102-${RUN_ID}/impl
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP02/lease-operations/impl
NAME=oo-a102-rust194-impl
NET=oo-a102-impl-net
CARGO_VOL=oo-a102-impl-cargo
TARGET_VOL=oo-a102-impl-target

mkdir -p "$EVIDENCE/phases" "$ROOT"
cp "$EVIDENCE/inside.sh" "$ROOT/inside.sh"
chmod 755 "$ROOT/inside.sh"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$EVIDENCE/builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n" | tee "$EVIDENCE/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
docker ps -a --filter label=oo.owner=a102 --filter label=oo.kind=rust-builder --format '{{.Names}} {{.Status}}' > "$EVIDENCE/own-builders-all.txt" || true
own=$(docker ps --filter label=oo.owner=a102 --filter label=oo.kind=rust-builder --format '{{.Names}}' || true)
if [ -n "$own" ]; then
  printf '%s\n' "$own" > "$EVIDENCE/deferred-own.txt"
  echo "deferred: this task already has a running rust builder" >&2
  exit 3
fi

rm -rf "$ROOT/snapshot"
mkdir -p "$ROOT/snapshot"
git -C "$REPO" archive "$COMMIT" | tar -C "$ROOT/snapshot" -xf -
printf '%s\n' "$COMMIT" > "$EVIDENCE/commit.txt"
printf '%s\n' "$TREE" > "$EVIDENCE/tree.txt"
printf '%s\n' "$PARENT" > "$EVIDENCE/parent.txt"
git -C "$REPO" rev-parse "${COMMIT}^{tree}" > "$EVIDENCE/archive-tree.txt"

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a102 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$NET" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a102 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a102 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
kind=implementation
commit=${COMMIT}
tree=${TREE}
parent=${PARENT}
source_baseline=aeb351cfb3b8c31e305568cd655ed057d4afdbf5
source_baseline_tree=62ca52ddfc358efb83dc032241938aa862775324
source_baseline_parent=4d59c8f467c3328d153113ed74f753369a3c054b
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
snapshot_method=git archive COMMIT
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
ignored_live=not_run
ignored_live_reason=mandated command has no --ignored; builder has no docker socket and no psql; fixture postgres is outside this network
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-jobs; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-jobs --all-targets -- -D warnings
EOF

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
  --label oo.owner=a102 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=lease-operations-impl-smoke \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp02-w01-b1 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" > "$EVIDENCE/inspect.txt" || true
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "$EVIDENCE/cap-drop.json" || true
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/ports.json" || true
docker cp "$NAME":/target/phases "$EVIDENCE/phases-from-container" || true
if [ -d "$EVIDENCE/phases-from-container" ]; then
  cp "$EVIDENCE/phases-from-container"/*.exit "$EVIDENCE/phases/" || true
fi
exit "$run_exit"
