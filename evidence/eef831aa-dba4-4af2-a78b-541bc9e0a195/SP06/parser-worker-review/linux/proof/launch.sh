#!/bin/bash
# One Linux arm64 Rust 1.94 proof builder. Starts only after the pre-overlay smoke.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a195/SP06/parser-worker-review
PROOF=${EVIDENCE}/linux/proof
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a195-eef831aa-dba4-4af2-a78b-541bc9e0a195
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=c22cf08aa5973ae2e2d95e0d2b72f98487f8555a
NAME=oo-a195-proof
NET=oo-a195-eef831aa-net
CARGO_VOL=oo-a195-eef831aa-cargo
TARGET_VOL=oo-a195-eef831aa-target

chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh"
mkdir -p "$PROOF/phases"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ "$(cat "$SMOKE/smoke.exit" 2>/dev/null || true)" != "0" ]; then
  echo "pre-overlay smoke did not exit 0" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ "$(cat "$SMOKE/docker-run.exit" 2>/dev/null || true)" != "0" ]; then
  echo "pre-overlay docker run did not exit 0" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/parser_worker_a195_review.rs" ]; then
  echo "immutable archive contains the review overlay" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/parser_worker.rs" ]; then
  echo "immutable archive is missing" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi

TREE=$ROOT/proof-src
if [ ! -f "$TREE/crates/oo-gateway/tests/parser_worker_a195_review.rs" ]; then
  rm -rf "$TREE"
  mkdir -p "$TREE"
  cp -a "$ROOT/archive/." "$TREE/" || {
    echo "archive copy failed" >&2
    printf '%s\n' 2 > "$PROOF/docker-run.exit"
    exit 2
  }
fi
cp "$EVIDENCE/overlay/parser_worker_a195_review.rs" "$TREE/crates/oo-gateway/tests/parser_worker_a195_review.rs" || {
  echo "overlay copy failed" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
}
if [ -f "$ROOT/archive/crates/oo-gateway/tests/parser_worker_a195_review.rs" ]; then
  echo "copy wrote the overlay into the immutable archive" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
want=$(git -C "$SRC" rev-parse "$FIXED:crates/oo-gateway/src/parser_worker.rs")
got=$(git -C "$SRC" hash-object "$TREE/crates/oo-gateway/src/parser_worker.rs")
printf 'parser_worker want=%s got=%s\n' "$want" "$got" > "$PROOF/product-blob.txt"
if [ "$want" != "$got" ]; then
  echo "proof tree parser_worker.rs is not the fixed blob" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
overlay_blob=$(git -C "$SRC" hash-object "$EVIDENCE/overlay/parser_worker_a195_review.rs")
tree_overlay=$(git -C "$SRC" hash-object "$TREE/crates/oo-gateway/tests/parser_worker_a195_review.rs")
printf 'overlay evidence=%s tree=%s\n' "$overlay_blob" "$tree_overlay" >> "$PROOF/product-blob.txt"
if [ "$overlay_blob" != "$tree_overlay" ]; then
  echo "proof tree overlay does not match the evidence overlay" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PROOF/host-cargo-procs.txt" || true
host_n=$(grep -c . "$PROOF/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$PROOF/builders-before.txt" || true
builder_n=$(grep -c . "$PROOF/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=6\n' "$host_n" "$builder_n" "$global_n" > "$PROOF/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a195 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$PROOF/image.txt"
grep -q 'arch=arm64' "$PROOF/image.txt"

cat > "$PROOF/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a195
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
postgres=none
phase=proof
global_cap=6
tree=${TREE}
fixed=${FIXED}
archive_overlay=absent
EOF

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e A195_ORACLE=/evidence/linux/proof/oracle.txt \
  --mount "type=bind,source=${TREE},target=/src,readonly" \
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a195 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a195 \
  --label oo.task=parser-worker-review-proof \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROOF/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROOF/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Tmpfs=$(docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$PROOF/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$PROOF/mounts.txt"
docker rm "$NAME" > "$PROOF/container-removed.txt"
exit "$run_exit"
