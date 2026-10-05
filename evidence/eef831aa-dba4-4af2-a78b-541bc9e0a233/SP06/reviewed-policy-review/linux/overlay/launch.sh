#!/bin/bash
# Fresh archive of baf70c5c plus only the review harness. Requires a green smoke. No PostgreSQL.
# Does not overwrite an existing console.log. Does not prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a233/SP06/reviewed-policy-review
OVERLAY=${EVIDENCE}/linux/overlay
ROOT=/tmp/oo-a233-eef831aa-dba4-4af2-a78b-541bc9e0a233
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=baf70c5c7b6d01183a178f53c77f65da4a7bb497
NAME=oo-a233-overlay
NET=oo-a233-eef831aa-net
CARGO_VOL=oo-a233-eef831aa-cargo
TARGET_VOL=oo-a233-eef831aa-target

chmod 755 "$OVERLAY/inside.sh" "$OVERLAY/launch.sh"
mkdir -p "$OVERLAY/phases" "$ROOT"
if [ -f "$OVERLAY/console.log" ]; then
  echo "refusing to overwrite overlay console.log" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi
if [ ! -f "$EVIDENCE/linux/smoke/smoke.exit" ] || [ "$(cat "$EVIDENCE/linux/smoke/smoke.exit")" != "0" ]; then
  echo "smoke is not green; refusing overlay" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$OVERLAY/docker-run.exit"
  exit 1
fi

git -C "$SRC" rev-parse "$FIXED" > "$OVERLAY/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$OVERLAY/fixed-tree.txt"
rm -rf "$ROOT/overlay"
mkdir -p "$ROOT/overlay"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/overlay"
cp "$EVIDENCE/overlay/reviewed_policy_a233_review.rs" \
  "$ROOT/overlay/crates/oo-gateway/tests/reviewed_policy_a233_review.rs"
cp "$EVIDENCE/overlay/reviewed_policy_a233_review.rs" "$OVERLAY/expect-review.rs"
if [ ! -f "$ROOT/overlay/crates/oo-gateway/src/reviewed_policy.rs" ]; then
  echo "archive missing reviewed_policy.rs" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi
git -C "$SRC" show "${FIXED}:rust-toolchain.toml" > "$OVERLAY/rust-toolchain.toml.fixed"
mv "$ROOT/overlay/rust-toolchain.toml" "$OVERLAY/rust-toolchain.toml.aside"
printf '%s\n' "moved aside; pinned image stays 1.94.0; git object unchanged" > "$OVERLAY/toolchain-aside.txt"
printf '%s\n' "harness=reviewed_policy_a233_review.rs archive=$FIXED" > "$OVERLAY/overlay-source.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a233 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a233 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$OVERLAY/network.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$OVERLAY/host-cargo-procs.txt" || true
host_n=$(grep -c . "$OVERLAY/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$OVERLAY/builders-before.txt" || true
builder_n=$(grep -c . "$OVERLAY/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$OVERLAY/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$OVERLAY/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a233 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$OVERLAY/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$OVERLAY/image.txt"
grep -q 'arch=arm64' "$OVERLAY/image.txt"
printf '%s\n' "$CARGO_VOL" > "$OVERLAY/volume-cargo.txt"
printf '%s\n' "$TARGET_VOL" > "$OVERLAY/volume-target.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named overlay container already exists" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi

cat > "$OVERLAY/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a233
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
postgres=not-started
phase=overlay
memory=6g
cpus=1
global_cap=10
archive=${ROOT}/overlay
fixed=${FIXED}
EOF

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/overlay,target=/src,readonly" \
  --mount "type=bind,source=${OVERLAY}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/source/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a233 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a233 \
  --label oo.task=reviewed-policy-review-overlay \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$OVERLAY/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$OVERLAY/docker-run.exit"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  {
    echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
    echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
    echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
    echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
    echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
    echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
    echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
    echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
    echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
    echo "Tmpfs=$(docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME")"
    echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
  } > "$OVERLAY/hostconfig.txt"
  docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$OVERLAY/mounts.txt"
  docker rm "$NAME" > "$OVERLAY/container-removed.txt"
fi
exit "$run_exit"
