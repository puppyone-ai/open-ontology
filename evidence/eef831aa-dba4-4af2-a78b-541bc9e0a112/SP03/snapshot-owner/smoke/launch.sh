#!/bin/bash
# Frozen baseline smoke before snapshot-owner edits. One owned Rust 1.94 builder.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a112
COMMIT=170dffba8375ca3631df0cc52fdbe65706b702a0
TREE=f090485c9fedc83d4b34af3eda3fc5732bde597e
PARENT=8b853d21bd66e5499baea07e8d3bb97232492d0c
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP03/snapshot-owner
ROOT=${EVIDENCE}/smoke/archive
NAME=${RUN_ID}-rust-base
NET=${RUN_ID}-net
CARGO_VOL=${RUN_ID}-cargo
TARGET_VOL=${RUN_ID}-target-base

mkdir -p "$EVIDENCE/smoke/phases" "$ROOT"
chmod 755 "$EVIDENCE/smoke/inside.sh" "$EVIDENCE/smoke/launch.sh"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/smoke/host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/smoke/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.ID}} {{.Names}}' > "$EVIDENCE/smoke/builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/smoke/builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n own_take=1 slot=A a107_handle_80154=absent" | tee "$EVIDENCE/smoke/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a112 --filter label=oo.kind=rust-builder --format '{{.ID}}' || true)
if [ -n "$own" ]; then
  echo "deferred: this task already has a running rust builder" >&2
  exit 3
fi

if [ ! -f "$ROOT/snapshot/Cargo.toml" ]; then
  rm -rf "$ROOT/snapshot"
  mkdir -p "$ROOT/snapshot"
  git -C "$REPO" archive "$COMMIT" | tar -C "$ROOT/snapshot" -xf -
fi
printf '%s\n' "$COMMIT" > "$EVIDENCE/smoke/commit.txt"
git -C "$REPO" rev-parse "${COMMIT}^{tree}" > "$EVIDENCE/smoke/tree.txt"
git -C "$REPO" rev-parse "${COMMIT}^" > "$EVIDENCE/smoke/parent.txt"
test "$(cat "$EVIDENCE/smoke/tree.txt")" = "$TREE"
test "$(cat "$EVIDENCE/smoke/parent.txt")" = "$PARENT"
git -C "$ROOT/snapshot" rev-parse HEAD >/dev/null 2>&1 || true
(cd "$ROOT/snapshot" && find . -type f ! -path './.git/*' -print0 | sort -z | xargs -0 shasum -a 256 | shasum -a 256 | awk '{print $1}') > "$EVIDENCE/smoke/snapshot.sha256"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a112 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=fixture-net \
  "$NET" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a112 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a112 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null

cat > "$EVIDENCE/smoke/invocation.txt" <<EOF
run_id=${RUN_ID}
kind=frozen-baseline-smoke
commit=${COMMIT}
tree=${TREE}
parent=${PARENT}
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
live_fixture=not-run
live_fixture_reason=baseline package pointers OO_PG_FIXTURE and OO_PG_TYPES_FIXTURE are not mounted
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors; cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
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
  --mount "type=bind,source=${EVIDENCE}/smoke/inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a112 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=snapshot-owner-frozen-smoke \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp03-r1 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/smoke/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/smoke/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} pid={{.State.Pid}}' > "$EVIDENCE/smoke/inspect-safe.txt" || true
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "$EVIDENCE/smoke/cap-drop.json" || true
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/smoke/ports.json" || true
mkdir -p "$EVIDENCE/smoke/phases-from-container"
docker cp "$NAME":/target/phases/. "$EVIDENCE/smoke/phases-from-container" || true
if [ -d "$EVIDENCE/smoke/phases-from-container" ]; then
  cp "$EVIDENCE/smoke/phases-from-container"/*.exit "$EVIDENCE/smoke/phases/" 2>/dev/null || true
  cp "$EVIDENCE/smoke/phases-from-container"/*.status "$EVIDENCE/smoke/phases/" 2>/dev/null || true
fi
exit "$run_exit"
