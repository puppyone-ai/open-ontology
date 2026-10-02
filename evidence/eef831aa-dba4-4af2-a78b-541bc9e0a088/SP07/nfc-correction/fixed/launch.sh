#!/bin/bash
# Linux arm64 rust 1.94 verification of the corrected builder snapshot.
# Capacity counts running rust builders and host cargo. No host cargo cache.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a088
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a088-fixed-${RUN_ID}
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP07/nfc-correction/fixed
NAME=oo-a088-rust194-fixed
NET=oo-a088-rust-net-fixed
CARGO_VOL=oo-a088-cargo-fixed
TARGET_VOL=oo-a088-target-fixed

mkdir -p "$EVIDENCE/phases" "$ROOT"
cp "$EVIDENCE/inside.sh" "$ROOT/inside.sh"
chmod 755 "$ROOT/inside.sh"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/host-cargo-procs.txt" || true)
{
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$EVIDENCE/builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n" | tee "$EVIDENCE/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a088 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  printf '%s\n' "$own" > "$EVIDENCE/deferred-own.txt"
  echo "deferred: this task already has a running rust builder" >&2
  exit 3
fi

COMMIT=$(git -C "$REPO" rev-parse HEAD)
TREE=$(git -C "$REPO" rev-parse "${COMMIT}^{tree}")
PARENT=$(git -C "$REPO" rev-parse "${COMMIT}^")
rm -rf "$ROOT/snapshot"
mkdir -p "$ROOT/snapshot"
git -C "$REPO" archive "$COMMIT" | tar -x -C "$ROOT/snapshot"
rsync -a --delete "$REPO/crates/oo-builder/" "$ROOT/snapshot/crates/oo-builder/"
cp "$REPO/Cargo.lock" "$ROOT/snapshot/Cargo.lock"
cp "$REPO/NOTICE" "$ROOT/snapshot/NOTICE"
cp "$REPO/contracts/builder/structure-draft/README.md" "$ROOT/snapshot/contracts/builder/structure-draft/README.md"
if [ -f "$ROOT/snapshot/crates/oo-builder/src/nfc_tables.rs" ]; then
  echo "overlay left nfc_tables.rs in place" >&2
  exit 2
fi
cat > "$ROOT/snapshot/SNAPSHOT.txt" <<EOF
base_commit=${COMMIT}
base_tree=${TREE}
base_parent=${PARENT}
overlay=worktree oo-builder Cargo.lock NOTICE contracts/builder/structure-draft/README.md
nfc_tables=absent
EOF
(
  cd "$ROOT/snapshot"
  find . -type f -print0 | sort -z | xargs -0 shasum -a 256
) > "$EVIDENCE/snapshot.sha256"

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a088 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$NET" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a088 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a088 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
base_commit=${COMMIT}
base_tree=${TREE}
base_parent=${PARENT}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
snapshot_method=git archive HEAD plus owned overlay
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
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings
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
  --label oo.owner=a088 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=nfc-correction-fixed \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp07-w01-b \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
docker_exit=$?
set -e
printf '%s\n' "$docker_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" > "$EVIDENCE/inspect.txt" || true
docker inspect --format '{{json .HostConfig}}' "$NAME" > "$EVIDENCE/hostconfig.json" || true
docker cp "$NAME":/target/phases/. "$EVIDENCE/phases" || true
exit "$docker_exit"
