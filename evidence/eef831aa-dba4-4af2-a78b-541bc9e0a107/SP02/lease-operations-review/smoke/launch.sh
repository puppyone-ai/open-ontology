#!/bin/bash
# One Linux arm64 Rust 1.94 smoke of archive c312cae. Not the live fixture.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a107
COMMIT=c312cae2daf75cfaa0b618066d709b9e529b3f95
TREE=19319751f04a7cb048bf3980a5408357d10acf58
PARENT=4477f23205e989ac3c2207098efc369e6286cb98
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP02/lease-operations-review
ROOT=${EVIDENCE}/archive
NAME=${RUN_ID}-rust
NET=${RUN_ID}-net
CARGO_VOL=${RUN_ID}-cargo
TARGET_VOL=${RUN_ID}-target

mkdir -p "$EVIDENCE/smoke/phases" "$ROOT"
chmod 755 "$EVIDENCE/smoke/inside.sh"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/smoke/host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/smoke/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.ID}}' > "$EVIDENCE/smoke/builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/smoke/builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n own_take=1 empty_does_not_grant_second=1" | tee "$EVIDENCE/smoke/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a107 --filter label=oo.kind=rust-builder --format '{{.ID}}' || true)
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

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a107 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=fixture-net \
  "$NET" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a107 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a107 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null

cat > "$EVIDENCE/smoke/invocation.txt" <<EOF
run_id=${RUN_ID}
kind=independent-review-smoke
commit=${COMMIT}
tree=${TREE}
parent=${PARENT}
handoff_commit=dd5ecf39604b8467a0d1f91d56d2e90473fa2c48
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
ignored_live_reason=author live test hardcodes a foreign container name; this smoke does not pass --ignored and does not mount a docker socket
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
  --mount "type=bind,source=${EVIDENCE}/smoke/inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a107 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=lease-operations-review-smoke \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp02-w01-b1 \
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
docker cp "$NAME":/target/phases "$EVIDENCE/smoke/phases-from-container" || true
if [ -d "$EVIDENCE/smoke/phases-from-container" ]; then
  cp "$EVIDENCE/smoke/phases-from-container"/*.exit "$EVIDENCE/smoke/phases/" || true
fi
exit "$run_exit"
