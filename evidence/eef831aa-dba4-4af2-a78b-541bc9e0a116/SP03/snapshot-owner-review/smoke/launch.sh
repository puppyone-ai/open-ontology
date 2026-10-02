#!/bin/bash
# One owned Rust 1.94 smoke builder. Slot A. Does not start PostgreSQL.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a116
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP03/snapshot-owner-review
SMOKE=${EVIDENCE}/smoke
ARCH=/tmp/oo-a116-${RUN_ID}/archive/snapshot
NAME=${RUN_ID}-rust-smoke
NET=${RUN_ID}-net
CARGO_VOL=${RUN_ID}-cargo
TARGET_VOL=${RUN_ID}-target

test -f "$ARCH/Cargo.toml"
test -f "$ARCH/crates/oo-connectors/src/snapshot.rs"
docker image inspect "$IMAGE" >/dev/null

ps -axo pid=,etime=,comm= | awk '$3 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "${SMOKE}/host-cargo-procs.txt" || true
host_n=$(grep -c . "${SMOKE}/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.ID}} {{.Names}} {{.Status}}' > "${SMOKE}/builders-before.txt" || true
builder_n=$(grep -c . "${SMOKE}/builders-before.txt" || true)
global_n=$((host_n + builder_n))
{
  echo "captured_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host_cargo=$host_n"
  echo "rust_builders=$builder_n"
  echo "global=$global_n"
  echo "own_take=1"
  echo "slot=A"
  echo "slot_b=a114_reserved"
  echo "a115_builder_grant=no"
  echo "a112_handle_50371=absent"
  echo "a114_handle_82080=absent"
  echo "a115_handle_82672=absent"
} | tee "${SMOKE}/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a116 --filter label=oo.kind=rust-builder --format '{{.ID}}' || true)
if [ -n "$own" ]; then
  echo "deferred: this review already has a running rust builder" >&2
  exit 3
fi

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a116 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a116 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a116 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null

if docker ps -a --filter "name=^/${NAME}$" --format '{{.ID}}' | grep -q .; then
  state=$(docker inspect -f '{{.State.Status}}' "$NAME")
  echo "refusing to replace existing ${NAME} status=${state}" >&2
  exit 1
fi

cat > "${SMOKE}/invocation.txt" <<EOF
run_id=${RUN_ID}
kind=frozen-smoke
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
source=${ARCH}
source_mount=read-only
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
command=cargo fetch --locked; cargo test --locked --offline -p oo-connectors --lib -- --test-threads=1
package_live=not-run
snapshot_live=not-run
EOF

chmod 755 "${SMOKE}/inside.sh" "${SMOKE}/launch.sh"
echo "smoke_start=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee "${SMOKE}/started.txt"

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
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${ARCH},target=/src,readonly" \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a116 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=snapshot-owner-review-smoke \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp03-r1-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "${SMOKE}/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "${SMOKE}/docker-run.exit"
echo "smoke_end=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "${SMOKE}/started.txt"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "${SMOKE}/inspect-safe.txt" || true
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "${SMOKE}/cap-drop.json" || true
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "${SMOKE}/ports.json" || true
mkdir -p "${SMOKE}/phases"
docker cp "$NAME":/target/phases/. "${SMOKE}/phases" || true
exit "$run_exit"
