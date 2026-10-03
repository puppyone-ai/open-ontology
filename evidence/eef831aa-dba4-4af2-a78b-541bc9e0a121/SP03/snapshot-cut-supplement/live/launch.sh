#!/bin/bash
# One owned Rust 1.94 review builder. PostgreSQL is already running.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a121
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP03/snapshot-cut-supplement
LIVE=${EVIDENCE}/live
ARCH=/tmp/oo-a121-${RUN_ID}/archive
SRC=/tmp/oo-a121-${RUN_ID}/src
SEC=/tmp/oo-a121-${RUN_ID}/secrets
ATTEMPT=${ATTEMPT:-1}
if [ "$ATTEMPT" = "1" ]; then
  NAME=${RUN_ID}-rust-review
  LOG=${LIVE}/console.log
  PHASE=${LIVE}/phases
else
  NAME=${RUN_ID}-rust-review-${ATTEMPT}
  LOG=${LIVE}/console-${ATTEMPT}.log
  PHASE=${LIVE}/phases-${ATTEMPT}
fi
SMOKE=${RUN_ID}-rust-smoke
PG=${RUN_ID}-pg
NET=${RUN_ID}-net
CARGO_VOL=${RUN_ID}-cargo
TARGET_VOL=${RUN_ID}-target
PGLOG=${RUN_ID}-pglog

test -f "$ARCH/Cargo.toml"
test -f "$ARCH/crates/oo-connectors/src/snapshot.rs"
test ! -e "$ARCH/crates/oo-connectors/tests/a121_oracle.rs"
test -f "${LIVE}/a121_oracle.rs"
test -f "${LIVE}/inside.sh"
test -f "${SEC}/pointer.txt"
test -f "${EVIDENCE}/expected/rows.txt"
test -f "${EVIDENCE}/product-sha256.txt"
docker image inspect "$IMAGE" >/dev/null
docker volume inspect "$PGLOG" >/dev/null
docker network inspect "$NET" >/dev/null

if ! docker exec -u postgres "$PG" pg_isready -U postgres -d lotdb >/dev/null 2>&1; then
  echo "postgres is not ready; refusing to take a rust builder" >&2
  exit 1
fi

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "${LIVE}/host-cargo-procs.txt" || true
host_n=$(grep -c . "${LIVE}/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.ID}} {{.Names}} {{.Status}}' > "${LIVE}/builders-before.txt" || true
builder_n=$(grep -c . "${LIVE}/builders-before.txt" || true)
global_n=$((host_n + builder_n))
{
  echo "captured_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "attempt=${ATTEMPT}"
  echo "host_cargo=$host_n"
  echo "rust_builders=$builder_n"
  echo "global=$global_n"
  echo "own_take=1"
  echo "slot=A"
  echo "slot_b=a120_handle_7304_live_vendor_writer"
} | tee "${LIVE}/capacity-${ATTEMPT}.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a121 --filter label=oo.kind=rust-builder --format '{{.ID}}' || true)
if [ -n "$own" ]; then
  echo "deferred: this review already has a running rust builder" >&2
  exit 3
fi
smoke_state=$(docker inspect -f '{{.State.Status}}' "$SMOKE" 2>/dev/null || echo absent)
if [ "$smoke_state" = "running" ]; then
  echo "deferred: smoke builder is still running" >&2
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.ID}}' | grep -q .; then
  state=$(docker inspect -f '{{.State.Status}}' "$NAME")
  echo "refusing to replace existing ${NAME} status=${state}" >&2
  exit 1
fi
if [ -e "$LOG" ]; then
  echo "refusing to overwrite ${LOG}" >&2
  exit 1
fi

if [ -e "$SRC" ]; then
  mv "$SRC" "${SRC}-kept-$(date -u +%Y%m%dT%H%M%SZ)"
fi
cp -a "$ARCH" "$SRC"
cp "${LIVE}/a121_oracle.rs" "$SRC/crates/oo-connectors/tests/a121_oracle.rs"
test ! -e "$ARCH/crates/oo-connectors/tests/a121_oracle.rs"
test -f "$SRC/crates/oo-connectors/tests/a121_oracle.rs"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null

cat > "${LIVE}/invocation-${ATTEMPT}.txt" <<EOF
run_id=${RUN_ID}
attempt=${ATTEMPT}
kind=independent-oracle
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
pglog_volume=${PGLOG}
source_archive=${ARCH}
source_copy=${SRC}
source_mount=read-only
probe=tests/a121_oracle.rs
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
command=sha256sum -c product; cargo test --locked --offline -p oo-connectors --test a121_oracle --no-run; cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings; cargo test --locked --offline -p oo-connectors --test a121_oracle -- --test-threads=1 --nocapture
lib_retest=not-run
package_live=not-run
snapshot_live=not-run
EOF

chmod 755 "${LIVE}/inside.sh" "${LIVE}/launch.sh"
echo "review_start=$(date -u +%Y-%m-%dT%H:%M:%SZ) attempt=${ATTEMPT}" | tee "${LIVE}/started-${ATTEMPT}.txt"

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
  -e OO_A121_FIXTURE=/secrets/pointer.txt \
  -e OO_A121_EXPECTED=/expected \
  -e OO_A121_OUTCOME=/target/phases/oracle-outcome.txt \
  --mount "type=bind,source=${SRC},target=/src,readonly" \
  --mount "type=bind,source=${LIVE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${EVIDENCE}/expected,target=/expected,readonly" \
  --mount "type=bind,source=${EVIDENCE}/product-sha256.txt,target=/evidence/product-sha256.txt,readonly" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a121 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=snapshot-cut-supplement \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp03-r1-supplement \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$LOG" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "${LIVE}/docker-run-${ATTEMPT}.exit"
echo "review_end=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "${LIVE}/started-${ATTEMPT}.txt"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "${LIVE}/inspect-${ATTEMPT}.txt" || true
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "${LIVE}/cap-drop-${ATTEMPT}.json" || true
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "${LIVE}/ports-${ATTEMPT}.json" || true
mkdir -p "$PHASE"
docker cp "$NAME":/target/phases/. "$PHASE" || true
exit "$run_exit"
