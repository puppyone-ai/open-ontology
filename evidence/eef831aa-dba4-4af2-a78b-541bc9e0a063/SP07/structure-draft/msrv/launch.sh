#!/bin/bash
set -eu
RUN_ID=136df3eb-c925-48f6-ab21-ab1c9bfe1a56
OLD_ID=ce854398-0ec1-4c91-bcdc-1c57421f7627
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a063-${RUN_ID}
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a063/SP07/structure-draft/msrv
OLD_NAME=oo-a063-smoke-${OLD_ID}
NEW_NAME=oo-a063-msrv-${RUN_ID}
NET=oo-a063-${RUN_ID}-net
CARGO_VOL=oo-a063-${RUN_ID}-cargo
TARGET_VOL=oo-a063-${RUN_ID}-target

mkdir -p "$EVIDENCE"
running=$(docker ps --filter label=oo.project=open-ontology --format '{{.Names}}')
if [ -n "$running" ]; then
  printf '%s\n' "$running" > "$EVIDENCE/deferred-running.txt"
  echo "deferred: another open-ontology builder is running" >&2
  exit 3
fi

owner=$(docker inspect "$OLD_NAME" --format '{{index .Config.Labels "oo.owner"}}')
session=$(docker inspect "$OLD_NAME" --format '{{index .Config.Labels "oo.session"}}')
old_run=$(docker inspect "$OLD_NAME" --format '{{index .Config.Labels "oo.run_id"}}')
task=$(docker inspect "$OLD_NAME" --format '{{index .Config.Labels "oo.task"}}')
if [ "$owner" != a063 ] || [ "$session" != eef831aa-dba4-4af2-a78b-541bc9e0a063 ] || [ "$old_run" != "$OLD_ID" ] || [ "$task" != structure-draft-pre-smoke ]; then
  echo "refusing to remove pre-smoke container; labels do not match" >&2
  exit 4
fi
docker rm "$OLD_NAME"
docker network rm "oo-a063-${OLD_ID}-net"
docker volume rm "oo-a063-${OLD_ID}-cargo" "oo-a063-${OLD_ID}-target"

mkdir -p "$ROOT/snapshot"
rsync -a --exclude target --exclude .git \
  "$SRC/Cargo.toml" "$SRC/Cargo.lock" "$SRC/rust-toolchain.toml" \
  "$ROOT/snapshot/"
rsync -a --exclude target --exclude .git "$SRC/crates/" "$ROOT/snapshot/crates/"
rsync -a --exclude target --exclude .git "$SRC/examples/" "$ROOT/snapshot/examples/"
rsync -a --exclude target --exclude .git "$SRC/contracts/" "$ROOT/snapshot/contracts/"
cp "$ROOT/inside.sh" "$EVIDENCE/inside.sh"

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NEW_NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
shared_cargo_cache=no
pull=never
command=cargo fetch --locked; cargo test --locked --offline -p oo-builder --all-targets; cargo clippy --locked --offline -p oo-builder --all-targets -- -D warnings
removed_pre_smoke=${OLD_NAME}
EOF

docker network create "$NET"
docker volume create "$CARGO_VOL" >/dev/null
docker volume create "$TARGET_VOL" >/dev/null

set +e
docker run \
  --name "$NEW_NAME" \
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
  --label oo.owner=a063 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a063 \
  --label oo.task=structure-draft-msrv \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp07-structure-draft-msrv \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NEW_NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt"
inspect_exit=$?
printf '%s\n' "$inspect_exit" > "$EVIDENCE/inspect.exit"
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
