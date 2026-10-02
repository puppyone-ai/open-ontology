#!/bin/bash
# Pre-edit focused baseline. One own builder. Does not prune or touch foreign containers.
set -eu
RUN_ID=3bf0c1b7-a66f-46ec-ac34-526cf13231f0
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a081-eef831aa-dba4-4af2-a78b-541bc9e0a081
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a081/SP07/structure-closure/baseline
NAME=oo-a081-msrv-${RUN_ID}
NET=oo-a081-${RUN_ID}-net
CARGO_VOL=oo-a081-${RUN_ID}-cargo
TARGET_VOL=oo-a081-${RUN_ID}-target

mkdir -p "$EVIDENCE" "$ROOT"
cd "$SRC"
if [ -n "$(git diff --cached --name-only)" ]; then
  echo "refusing: index has staged paths" >&2
  git diff --cached --name-only >&2
  exit 2
fi
BASELINE=$(git rev-parse HEAD)
git rev-parse HEAD > "$EVIDENCE/baseline-head.txt"
git rev-parse "${BASELINE}^{tree}" > "$EVIDENCE/baseline-tree.txt"
git rev-parse "${BASELINE}^" > "$EVIDENCE/baseline-parent.txt"
git diff --stat 4c989bce58407cf19d3d93dc600162d7322d92f8 "$BASELINE" -- crates/oo-builder contracts/builder/structure-draft examples/ontology-draft > "$EVIDENCE/builder-vs-reviewed.diffstat"

docker ps --format 'name={{.Names}} image={{.Image}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -ax -o pid=,comm= > "$EVIDENCE/processes-before.txt"

builders=$(docker ps --filter ancestor="$IMAGE" -q | sed '/^$/d' | wc -l | tr -d ' ')
own=$(docker ps -q --filter label=oo.owner=a081 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -ax -o comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc"' | wc -l | tr -d ' ')
printf 'builders=%s own=%s host_cargo=%s\n' "$builders" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi
if [ "$((builders + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi

rm -rf "$ROOT/snapshot"
mkdir -p "$ROOT/snapshot"
git -C "$SRC" archive --format=tar "$BASELINE" | tar -C "$ROOT/snapshot" -xf -
cp "$EVIDENCE/inside.sh" "$ROOT/inside.sh"
chmod 755 "$ROOT/inside.sh"
(
  cd "$ROOT/snapshot"
  find . -type f | sort | shasum -a 256
) > "$EVIDENCE/archive-inputs.sha256"
shasum -a 256 "$EVIDENCE/inside.sh" > "$EVIDENCE/inside.sha256"
git -C "$SRC" rev-parse "${BASELINE}:Cargo.lock" > "$EVIDENCE/baseline-lock-blob.txt"
git -C "$SRC" rev-parse "${BASELINE}:crates/oo-builder/src/draft.rs" > "$EVIDENCE/baseline-draft-blob.txt"
git -C "$SRC" rev-parse "${BASELINE}:crates/oo-builder/src/pgsql.rs" > "$EVIDENCE/baseline-pgsql-blob.txt"

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
baseline_head=${BASELINE}
baseline_parent=$(cat "$EVIDENCE/baseline-parent.txt")
baseline_tree=$(cat "$EVIDENCE/baseline-tree.txt")
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
network_mode=bridge_not_host
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
snapshot_method=git archive of the pinned commit
privileged=no
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
cap_drop=ALL
cap_add=none
init=yes
no_new_privileges=yes
memory=8g
memory_swap=8g
cpus=4
pids_limit=4096
user=0:0
tmpfs=/tmp:rw,nosuid,nodev,size=1g
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings
EOF

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a081 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a081 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=task-net \
  "$NET"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a081 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a081 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=task-cargo \
  "$CARGO_VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a081 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a081 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=task-target \
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
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  --workdir /src \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${ROOT}/snapshot,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a081 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a081 \
  --label oo.task=structure-closure-baseline \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp07-structure-closure \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" > "$EVIDENCE/inspect.json"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}} ReadonlyRootfs={{.HostConfig.ReadonlyRootfs}} CapDrop={{json .HostConfig.CapDrop}} CapAdd={{json .HostConfig.CapAdd}} User={{.Config.User}} SecurityOpt={{json .HostConfig.SecurityOpt}} Tmpfs={{json .HostConfig.Tmpfs}}' > "$EVIDENCE/inspect.txt"
docker inspect "$NAME" --format '{{json .Mounts}}' > "$EVIDENCE/mounts.json"
docker inspect "$NAME" --format '{{json .HostConfig.Binds}}' > "$EVIDENCE/binds.json"
docker inspect "$NAME" --format '{{json .Config.Labels}}' > "$EVIDENCE/labels.json"
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
