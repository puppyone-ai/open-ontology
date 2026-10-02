#!/bin/bash
# One owned Linux arm64 rust 1.94 builder for the sealed baseline archive.
# Capacity: pid/ppid/etime/comm only. No host cargo cache, no host network.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a088
COMMIT=39376ac1e68c0ee066f1c46cd8c7144ea598ef52
TREE=b45b5a767eef0bab04d75b942ec22701946ba5ae
PARENT=99e00edffd232cbeae95f374568c9ea537cfb478
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a088-${RUN_ID}
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/${RUN_ID}/SP07/nfc-correction/baseline
NAME=oo-a088-rust194-baseline
NET=oo-a088-rust-net-baseline
CARGO_VOL=oo-a088-cargo-baseline
TARGET_VOL=oo-a088-target-baseline

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
own=$(docker ps -a --filter label=oo.owner=a088 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  printf '%s\n' "$own" > "$EVIDENCE/deferred-own.txt"
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi

if [ ! -d "$ROOT/snapshot" ]; then
  mkdir -p "$ROOT/snapshot"
  git -C /Users/redux/puppy-one/open-ontology archive "$COMMIT" | tar -x -C "$ROOT/snapshot"
fi
(
  cd "$ROOT/snapshot"
  find . -type f -print0 | sort -z | xargs -0 shasum -a 256
) > "$EVIDENCE/snapshot.sha256"
git -C /Users/redux/puppy-one/open-ontology rev-parse "${COMMIT}^{tree}" > "$EVIDENCE/archive-tree.txt"
printf '%s\n' "$COMMIT" > "$EVIDENCE/archive-commit.txt"

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
  --label oo.task=nfc-correction-baseline \
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
docker inspect "$NAME" > "$EVIDENCE/inspect.txt"
docker inspect --format '{{json .HostConfig}}' "$NAME" > "$EVIDENCE/hostconfig.json"
docker cp "$NAME":/target/phases/. "$EVIDENCE/phases" || true
exit "$docker_exit"
