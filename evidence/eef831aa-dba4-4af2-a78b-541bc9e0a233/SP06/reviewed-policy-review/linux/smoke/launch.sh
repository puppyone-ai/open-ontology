#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Immutable archive of baf70c5c. No review harness. No PostgreSQL.
# Does not overwrite an existing console.log. Does not prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a233/SP06/reviewed-policy-review
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a233-eef831aa-dba4-4af2-a78b-541bc9e0a233
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PGIMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
FIXED=baf70c5c7b6d01183a178f53c77f65da4a7bb497
NAME=oo-a233-smoke
NET=oo-a233-eef831aa-net
CARGO_VOL=oo-a233-eef831aa-cargo
TARGET_VOL=oo-a233-eef831aa-target

chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh" "$SMOKE/extract-libclang.sh"
mkdir -p "$SMOKE/phases" "$ROOT"
if [ -f "$SMOKE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$SMOKE/docker-run.exit"
  exit 1
fi
if docker image inspect "$PGIMAGE" >/dev/null 2>&1; then
  docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$PGIMAGE" > "$SMOKE/postgres-image.txt"
else
  printf '%s\n' "postgres image not inspected; not pulled" > "$SMOKE/postgres-image.txt"
fi
printf '%s\n' "not-started" > "$SMOKE/postgres.txt"

git -C "$SRC" rev-parse HEAD > "$SMOKE/repo-head.txt"
git -C "$SRC" rev-parse "$FIXED" > "$SMOKE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$SMOKE/fixed-tree.txt"
rm -rf "$ROOT/smoke"
mkdir -p "$ROOT/smoke"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/smoke"
if [ ! -f "$ROOT/smoke/crates/oo-gateway/src/reviewed_policy.rs" ]; then
  echo "archive missing reviewed_policy.rs" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/smoke/crates/oo-gateway/tests/reviewed_policy_a233_review.rs" ]; then
  echo "archive already contains the review harness" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
git -C "$SRC" show "${FIXED}:rust-toolchain.toml" > "$SMOKE/rust-toolchain.toml.fixed"
mv "$ROOT/smoke/rust-toolchain.toml" "$SMOKE/rust-toolchain.toml.aside"
printf '%s\n' "moved aside; pinned image stays 1.94.0; git object unchanged" > "$SMOKE/toolchain-aside.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a233 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a233 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$SMOKE/network.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$SMOKE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$SMOKE/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$SMOKE/builders-before.txt" || true
builder_n=$(grep -c . "$SMOKE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$SMOKE/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a233 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a233 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a233 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a233 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a233 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
printf '%s\n' "$CARGO_VOL" > "$SMOKE/volume-cargo.txt"
printf '%s\n' "$TARGET_VOL" > "$SMOKE/volume-target.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi

cat > "$SMOKE/invocation.txt" <<EOF
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
phase=smoke
memory=6g
cpus=1
global_cap=10
archive=${ROOT}/smoke
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
  --mount "type=bind,source=${ROOT}/smoke,target=/src,readonly" \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE}/source/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a233 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a233 \
  --label oo.task=reviewed-policy-review-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$SMOKE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$SMOKE/docker-run.exit"
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
  } > "$SMOKE/hostconfig.txt"
  docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$SMOKE/mounts.txt"
  docker rm "$NAME" > "$SMOKE/container-removed.txt"
fi
exit "$run_exit"
