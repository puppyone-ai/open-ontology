#!/bin/bash
set -eu
RUN_ID=$(uuidgen | tr 'A-Z' 'a-z')
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a073-${RUN_ID}
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a073/SP09/sdk-json-query/linux-post
NAME=oo-a073-msrv-${RUN_ID}
NET=oo-a073-${RUN_ID}-net
CARGO_VOL=oo-a073-${RUN_ID}-cargo
TARGET_VOL=oo-a073-${RUN_ID}-target
HEAD=$(git -C "$SRC" rev-parse HEAD)
PARENT=$(git -C "$SRC" rev-parse "${HEAD}^")
TREE=$(git -C "$SRC" rev-parse "${HEAD}^{tree}")

mkdir -p "$EVIDENCE"
printf '%s\n' "$RUN_ID" > "$EVIDENCE/run-id.txt"
running=$(docker ps --filter label=oo.project=open-ontology --filter label=oo.kind=rust-builder --format '{{.Names}}')
count=$(printf '%s\n' "$running" | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps aux | grep -E '[c]argo ' | wc -l | tr -d ' ')
if [ "$count" -ge 1 ]; then
  printf '%s\n' "$running" > "$EVIDENCE/deferred-running.txt"
  echo "deferred: an open-ontology rust builder is already running" >&2
  exit 3
fi
if [ "$((count + host_cargo))" -ge 2 ]; then
  ps aux | grep -E '[c]argo |[r]ustc ' > "$EVIDENCE/deferred-host.txt"
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi

mkdir -p "$ROOT/snapshot"
git -C "$SRC" archive --format=tar "$HEAD" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo \
  | tar -C "$ROOT/snapshot" -xf -
rm -rf "$ROOT/snapshot/crates/oo-sdk"
cp -R "$SRC/crates/oo-sdk" "$ROOT/snapshot/crates/oo-sdk"
cp "$SRC/Cargo.lock" "$ROOT/snapshot/Cargo.lock"
mkdir -p "$ROOT/snapshot/contracts/sdk"
rm -rf "$ROOT/snapshot/contracts/sdk/json-query"
cp -R "$SRC/contracts/sdk/json-query" "$ROOT/snapshot/contracts/sdk/json-query"
cp "$EVIDENCE/inside.sh" "$ROOT/inside.sh"
printf '%s\n' "$HEAD" > "$EVIDENCE/baseline-head.txt"
printf '%s\n' "$TREE" > "$EVIDENCE/baseline-tree.txt"
printf '%s\n' "$PARENT" > "$EVIDENCE/baseline-parent.txt"
find "$ROOT/snapshot" -type f | sort | shasum -a 256 > "$EVIDENCE/snapshot-files.sha256"
shasum -a 256 "$EVIDENCE/inside.sh" > "$EVIDENCE/inside.sha256"

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
baseline_head=${HEAD}
baseline_parent=${PARENT}
baseline_tree=${TREE}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
snapshot_method=git archive of HEAD workspace inputs, then overlay working Cargo.lock, crates/oo-sdk, and contracts/sdk/json-query
snapshot_note=contracts/sdk/json-query is included because oo-sdk tests compile those fixtures with include_str/include_bytes. Sibling dirty trees are not overlaid. The pinned image has gcc and no cmake; aws-lc-sys 0.45.0 is expected to use its cc builder and pregenerated bindings for aarch64-unknown-linux-gnu.
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
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-sdk; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-sdk --all-targets -- -D warnings
EOF

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a073 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a073 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$NET"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a073 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a073 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a073 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a073 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null

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
  --label oo.owner=a073 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a073 \
  --label oo.task=sdk-json-query-linux-post \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp09-sdk-json-query \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt"
docker cp "$NAME":/target/phases "$EVIDENCE/phases" || true
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
