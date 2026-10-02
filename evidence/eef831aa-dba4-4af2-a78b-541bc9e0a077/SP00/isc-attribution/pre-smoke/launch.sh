#!/bin/bash
# Pre-edit focused check. One own builder. Does not prune or touch foreign containers.
set -eu
RUN_ID=e844e982-ba5c-462c-b01a-27d3c63bf811
BASELINE=6fa6481267e5a4efac20831fa0b407a8cf028905
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a077-${RUN_ID}
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/pre-smoke
NAME=oo-a077-msrv-${RUN_ID}
NET=oo-a077-${RUN_ID}-net
CARGO_VOL=oo-a077-${RUN_ID}-cargo
TARGET_VOL=oo-a077-${RUN_ID}-target

mkdir -p "$EVIDENCE"
docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -ax -o pid,comm,command > "$EVIDENCE/processes-before.txt"

builders=$(docker ps -q --filter label=oo.project=open-ontology --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own=$(docker ps -aq --filter label=oo.owner=a077 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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

mkdir -p "$ROOT/snapshot"
git -C "$SRC" archive --format=tar "$BASELINE" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo \
  | tar -C "$ROOT/snapshot" -xf -
cp "$EVIDENCE/inside.sh" "$ROOT/inside.sh"
chmod 755 "$ROOT/inside.sh"
git -C "$SRC" rev-parse "$BASELINE" > "$EVIDENCE/baseline-head.txt"
git -C "$SRC" rev-parse "${BASELINE}^{tree}" > "$EVIDENCE/baseline-tree.txt"
git -C "$SRC" rev-parse "${BASELINE}^" > "$EVIDENCE/baseline-parent.txt"
(
  cd "$ROOT/snapshot"
  find . -type f | sort | shasum -a 256
) > "$EVIDENCE/archive-inputs.sha256"
shasum -a 256 "$EVIDENCE/inside.sh" > "$EVIDENCE/inside.sha256"
git -C "$SRC" rev-parse "${BASELINE}:Cargo.lock" > "$EVIDENCE/baseline-lock-blob.txt"
git -C "$SRC" rev-parse "${BASELINE}:ci/deny.toml" > "$EVIDENCE/baseline-deny-blob.txt"
shasum -a 256 \
  "$SRC/ci/deny.toml" \
  "$SRC/Cargo.toml" \
  "$SRC/Cargo.lock" \
  "$SRC/supply-chain/policy.md" \
  "$SRC/contracts/supply-chain/isc/NOTICE.md" \
  "$SRC/contracts/supply-chain/isc/policy.md" \
  "$SRC/contracts/supply-chain/isc/provenance.md" \
  > "$EVIDENCE/pre-edit-hashes.txt"

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
snapshot_method=git archive of the pinned commit, workspace inputs only
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
command=cargo fetch --manifest-path Cargo.toml --locked; cargo check --manifest-path Cargo.toml --locked --offline -p oo-core
EOF

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a077 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a077 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=task-net \
  "$NET"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a077 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a077 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=task-cargo \
  "$CARGO_VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a077 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a077 \
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
  --label oo.owner=a077 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a077 \
  --label oo.task=isc-attribution-pre-smoke \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp00-isc-attribution \
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
