#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Pre-implementation smoke. No PostgreSQL.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh
# Change: owner a328, 6GiB, 1 CPU, no archive compile, fixed 8c2005d5, global cap 10.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder
SMOKE=${EVIDENCE}/linux/smoke
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8
NAME=oo-a328-smoke
NET=oo-a328-eef831aa-net
CARGO_VOL=oo-a328-eef831aa-cargo
TARGET_VOL=oo-a328-eef831aa-target

chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh"
mkdir -p "$SMOKE/phases"
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

git -C "$SRC" rev-parse "$FIXED" > "$SMOKE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$SMOKE/archive-tree.txt"
git -C "$SRC" ls-tree -r --name-only "$FIXED" -- tools/pg-builder testing/real-db/pg-builder > "$SMOKE/owned-in-archive.txt"
if [ -s "$SMOKE/owned-in-archive.txt" ]; then
  echo "fixed archive already contains the owned overlay" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
{
  printf 'a200 %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh)"
  printf 'a204 %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh)"
  printf 'a199 %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh)"
} > "$SMOKE/recipe-origins.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a328 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a328 \
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
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a328 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a328 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a328 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a328 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a328 \
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
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a328
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
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
phase=preoverlay
global_cap=10
memory=6g
cpus=1
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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a328 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a328 \
  --label oo.task=pg-builder-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$SMOKE/console.log" 2>&1
run_status=$?
set -e
printf '%s\n' "$run_status" > "$SMOKE/docker-run.exit"
docker inspect --format 'oom={{.State.OOMKilled}} status={{.State.Status}} exit={{.State.ExitCode}} mem={{.HostConfig.Memory}} cpu={{.HostConfig.NanoCpus}} priv={{.HostConfig.Privileged}} net={{.HostConfig.NetworkMode}} pidslimit={{.HostConfig.PidsLimit}}' "$NAME" > "$SMOKE/hostconfig.txt"
docker inspect --format '{{json .HostConfig.Binds}}' "$NAME" > "$SMOKE/mounts.txt"
docker inspect --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' "$NAME" > "$SMOKE/caps.txt"
docker rm "$NAME" >/dev/null
printf '%s\n' removed > "$SMOKE/container-removed.txt"
exit "$run_status"
