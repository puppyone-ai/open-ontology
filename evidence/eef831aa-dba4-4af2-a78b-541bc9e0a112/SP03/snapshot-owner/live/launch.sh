#!/bin/bash
# One owned Rust 1.94 builder over the frozen archive plus the owned patch.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a112
COMMIT=170dffba8375ca3631df0cc52fdbe65706b702a0
TREE=f090485c9fedc83d4b34af3eda3fc5732bde597e
PARENT=8b853d21bd66e5499baea07e8d3bb97232492d0c
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP03/snapshot-owner
LIVE=${EVIDENCE}/live
FROZEN=${EVIDENCE}/smoke/archive/snapshot
PATCH=${LIVE}/patch
NAME=${RUN_ID}-rust-r1
NET=${RUN_ID}-net
CARGO_VOL=${RUN_ID}-cargo
TARGET_VOL=${RUN_ID}-target-r1
SRC_VOL=${RUN_ID}-src-r1
SEC=/tmp/oo-a112-${RUN_ID}
PG=${RUN_ID}-pg
PGLOG=${RUN_ID}-pglog

test -f "$FROZEN/Cargo.toml"
test -f "${LIVE}/pointer-path.txt"
test -f "$SEC/pointer.json"
docker inspect "$PG" >/dev/null
running=$(docker inspect -f '{{.State.Running}}' "$PG")
test "$running" = "true"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "${LIVE}/host-cargo-procs.txt" || true
host_n=$(grep -c . "${LIVE}/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.ID}} {{.Names}}' > "${LIVE}/builders-before.txt" || true
builder_n=$(grep -c . "${LIVE}/builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n own_take=1 slot=A handle_5564=absent handle_80154=absent" | tee "${LIVE}/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a112 --filter label=oo.kind=rust-builder --format '{{.ID}}' || true)
if [ -n "$own" ]; then
  echo "deferred: this task already has a running rust builder" >&2
  exit 3
fi

rm -rf "$PATCH"
mkdir -p \
  "$PATCH/crates/oo-connectors/src/types" \
  "$PATCH/crates/oo-connectors/tests" \
  "$PATCH/contracts/source/pgsql"
cp "$REPO/crates/oo-connectors/src/session.rs" "$PATCH/crates/oo-connectors/src/session.rs"
cp "$REPO/crates/oo-connectors/src/error.rs" "$PATCH/crates/oo-connectors/src/error.rs"
cp "$REPO/crates/oo-connectors/src/sql.rs" "$PATCH/crates/oo-connectors/src/sql.rs"
cp "$REPO/crates/oo-connectors/src/lib.rs" "$PATCH/crates/oo-connectors/src/lib.rs"
cp "$REPO/crates/oo-connectors/src/snapshot.rs" "$PATCH/crates/oo-connectors/src/snapshot.rs"
cp "$REPO/crates/oo-connectors/src/types/mod.rs" "$PATCH/crates/oo-connectors/src/types/mod.rs"
cp "$REPO/crates/oo-connectors/tests/snapshot_live.rs" "$PATCH/crates/oo-connectors/tests/snapshot_live.rs"
cp "$REPO/crates/oo-connectors/README.md" "$PATCH/crates/oo-connectors/README.md"
cp "$REPO/contracts/source/pgsql/snapshot-owner-v1.md" "$PATCH/contracts/source/pgsql/snapshot-owner-v1.md"

docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a112 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null
docker volume inspect "$SRC_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a112 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$SRC_VOL" >/dev/null

if docker ps -a --filter "name=^/${NAME}$" --format '{{.ID}}' | grep -q .; then
  state=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$state" = "running" ]; then
    echo "deferred: rust-r1 is already running" >&2
    exit 3
  fi
  echo "refusing to remove existing ${NAME} status=${state}" >&2
  exit 1
fi

cat > "${LIVE}/invocation.txt" <<EOF
run_id=${RUN_ID}
kind=r1-overlay
frozen_commit=${COMMIT}
frozen_tree=${TREE}
frozen_parent=${PARENT}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
src_volume=${SRC_VOL}
snapshot=${FROZEN}
snapshot_method=frozen archive plus owned patch
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
pg_container=${PG}
command=cargo test --locked --offline -p oo-connectors --lib --test snapshot_live -- --test-threads=1; cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings
EOF

chmod 755 "${LIVE}/inside.sh" "${LIVE}/launch.sh" "${LIVE}/pg-wrap.sh" "${LIVE}/init.sh" "${LIVE}/up.sh"

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
  -e OO_PG_SNAPSHOT_FIXTURE=/secrets/pointer.json \
  -e OO_PG_SNAPSHOT_OUTCOME=/target/phases/outcome.txt \
  --mount "type=bind,source=${FROZEN},target=/frozen,readonly" \
  --mount "type=bind,source=${PATCH},target=/patch,readonly" \
  --mount "type=bind,source=${LIVE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=volume,source=${SRC_VOL},target=/src" \
  --label oo.project=open-ontology \
  --label oo.owner=a112 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=snapshot-owner-r1 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp03-r1 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "${LIVE}/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "${LIVE}/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} pid={{.State.Pid}}' > "${LIVE}/inspect-safe.txt" || true
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "${LIVE}/cap-drop.json" || true
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "${LIVE}/ports.json" || true
mkdir -p "${LIVE}/phases-from-container"
docker cp "$NAME":/target/phases/. "${LIVE}/phases-from-container" || true
mkdir -p "${LIVE}/phases"
if [ -d "${LIVE}/phases-from-container" ]; then
  cp "${LIVE}/phases-from-container"/*.exit "${LIVE}/phases/" 2>/dev/null || true
  cp "${LIVE}/phases-from-container"/*.status "${LIVE}/phases/" 2>/dev/null || true
  cp "${LIVE}/phases-from-container"/outcome.txt "${LIVE}/phases/" 2>/dev/null || true
fi
exit "$run_exit"
