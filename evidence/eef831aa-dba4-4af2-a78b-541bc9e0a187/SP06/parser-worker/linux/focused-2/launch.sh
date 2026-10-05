#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Copied archive plus the parser-worker overlay.
# Attempt 1 failed because new files cannot be bind-mounted onto a read-only archive.
# Reuses the pre-edit cargo and target volumes. No PostgreSQL.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a187/SP06/parser-worker
FOCUS=${EVIDENCE}/linux/focused-2
ROOT=/tmp/oo-a187-eef831aa-dba4-4af2-a78b-541bc9e0a187
TREE=${ROOT}/focused-tree
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a187-focused-2
NET=oo-a187-net
CARGO_VOL=oo-a187-cargo
TARGET_VOL=oo-a187-target

chmod 755 "$FOCUS/inside.sh" "$FOCUS/launch.sh"
mkdir -p "$FOCUS/phases"
if [ -f "$FOCUS/console.log" ]; then
  echo "refusing to overwrite focused-2 console.log" >&2
  printf '%s\n' 2 > "$FOCUS/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/sql_guard.rs" ]; then
  echo "pre-edit archive is absent" >&2
  printf '%s\n' 2 > "$FOCUS/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/src/parser_worker.rs" ]; then
  echo "archive already contains parser_worker.rs" >&2
  printf '%s\n' 2 > "$FOCUS/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$FOCUS/docker-run.exit"
  exit 1
fi

git -C "$SRC" rev-parse HEAD > "$FOCUS/archive-head.txt"
git -C "$SRC" merge-base --is-ancestor 8a289f0c699abb8c707ca45ad45cb036abc3fc05 HEAD
echo "ancestor_8a289f0c=yes" >> "$FOCUS/archive-head.txt"
printf '%s\n' "523951fd9bea54e38c9e6c14a2df109cf0b1b196" > "$FOCUS/archive-commit.txt"

hash_one() {
  sha256sum "$1" | awk '{print $1}'
}
{
  for rel in \
    Cargo.lock \
    Cargo.toml \
    crates/oo-gateway/Cargo.toml \
    crates/oo-gateway/src/sql_guard.rs \
    crates/oo-gateway/src/query_executor.rs \
    crates/oo-gateway/src/policy.rs
  do
    archive_sum=$(hash_one "$ROOT/archive/$rel")
    work_sum=$(hash_one "$SRC/$rel")
    if [ "$archive_sum" != "$work_sum" ]; then
      echo "forbidden file differs from the archive: $rel" >&2
      printf '%s\n' 2 > "$FOCUS/docker-run.exit"
      exit 2
    fi
    printf '%s  %s\n' "$archive_sum" "$rel"
  done
  for rel in \
    crates/oo-gateway/src/parser_worker.rs \
    crates/oo-gateway/src/lib.rs \
    crates/oo-gateway/src/bin/oo-sql-parser-worker.rs \
    crates/oo-gateway/tests/parser_worker.rs \
    contracts/gateway/parser-worker-v1.md
  do
    printf '%s  %s\n' "$(hash_one "$SRC/$rel")" "$rel"
  done
} > "$FOCUS/expected.sha256"
hash_one "$ROOT/archive/crates/oo-control/src/file_hosting.rs" > "$FOCUS/archive-control.sha256"
hash_one "$SRC/crates/oo-control/src/file_hosting.rs" > "$FOCUS/worktree-control.sha256"

echo "copying archive to ${TREE}"
rm -rf "$TREE"
mkdir -p "$TREE"
cp -a "$ROOT/archive/." "$TREE/"
for rel in \
  crates/oo-gateway/src/parser_worker.rs \
  crates/oo-gateway/src/lib.rs \
  crates/oo-gateway/src/bin/oo-sql-parser-worker.rs \
  crates/oo-gateway/tests/parser_worker.rs \
  contracts/gateway/parser-worker-v1.md
do
  mkdir -p "$TREE/$(dirname "$rel")"
  cp "$SRC/$rel" "$TREE/$rel"
done
if [ ! -f "$TREE/crates/oo-gateway/src/parser_worker.rs" ]; then
  echo "overlay copy missed parser_worker.rs" >&2
  printf '%s\n' 2 > "$FOCUS/docker-run.exit"
  exit 2
fi
if ! cmp -s "$TREE/crates/oo-control/src/file_hosting.rs" "$ROOT/archive/crates/oo-control/src/file_hosting.rs"; then
  echo "copied control file differs from the archive" >&2
  printf '%s\n' 2 > "$FOCUS/docker-run.exit"
  exit 2
fi

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a187 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a187 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$FOCUS/network.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$FOCUS/host-cargo-procs.txt" || true
host_n=$(grep -c . "$FOCUS/host-cargo-procs.txt" || true)
{
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$FOCUS/containers-before.txt" || true
builder_n=$(grep -c . "$FOCUS/containers-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=3\n' "$host_n" "$builder_n" "$global_n" > "$FOCUS/capacity.txt"
if [ "$global_n" -ge 3 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$FOCUS/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter label=oo.owner=a187 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$FOCUS/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$FOCUS/image.txt"
grep -q 'arch=arm64' "$FOCUS/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
printf '%s\n' "$CARGO_VOL" > "$FOCUS/volume-cargo.txt"
printf '%s\n' "$TARGET_VOL" > "$FOCUS/volume-target.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named focused container already exists" >&2
  printf '%s\n' 2 > "$FOCUS/docker-run.exit"
  exit 2
fi

cat > "$FOCUS/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a187
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
phase=focused-2
global_cap=3
archive=${ROOT}/archive
tree=${TREE}
overlay=copied-parser-worker
prior_attempt=linux/focused docker-run.exit=125 read-only-mountpoint
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
  -e CARGO_NET_OFFLINE=true \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${TREE},target=/src,readonly" \
  --mount "type=bind,source=${FOCUS}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${FOCUS}/expected.sha256,target=/expected.sha256,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a187 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a187 \
  --label oo.task=parser-worker-focused-2 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$FOCUS/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$FOCUS/docker-run.exit"
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
} > "$FOCUS/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$FOCUS/mounts.txt"
docker rm "$NAME" > "$FOCUS/container-removed.txt"
exit "$run_exit"
