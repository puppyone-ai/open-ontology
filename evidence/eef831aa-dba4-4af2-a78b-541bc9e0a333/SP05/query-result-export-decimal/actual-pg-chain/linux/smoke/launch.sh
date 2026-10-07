#!/bin/bash
# Focused smoke of exporter commit 62b4afec. No moving worktree mount.
# Origin: evidence/.../a333/.../linux/smoke/launch.sh and a340 proof launch.
# Changes: archive is only tools/query-result-export at 62b4afec. 6GiB, 1 CPU.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a333/SP05/query-result-export-decimal
LIVE=${REVIEW}/actual-pg-chain/linux/smoke
ROOT=/tmp/oo-a333-eef831aa-dba4-4af2-a78b-541bc9e0a333
EXPORTER=62b4afecdd377fa94310f46467c48a01e61d14db
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a333-export-smoke
NET=oo-a333-eef831aa-net
CARGO_VOL=oo-a333-eef831aa-cargo
TARGET_VOL=oo-a333-eef831aa-target

chmod 755 "$LIVE/inside.sh" "$LIVE/launch.sh"
mkdir -p "$LIVE/phases" "$ROOT"
if [ -f "$LIVE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$LIVE/docker-run.exit"
  exit 1
fi
git -C "$SRC" rev-parse "$EXPORTER" > "$LIVE/exporter-commit.txt"
if git -C "$SRC" diff --quiet "$EXPORTER" HEAD -- tools/query-result-export; then
  printf '%s\n' "worktree_matches_62b4afec" > "$LIVE/worktree.txt"
else
  printf '%s\n' "worktree_differs_archive_still_62b4afec" > "$LIVE/worktree.txt"
fi
rm -rf "$ROOT/export-archive"
mkdir -p "$ROOT/export-archive"
git -C "$SRC" archive --format=tar "$EXPORTER" tools/query-result-export | tar -xf - -C "$ROOT/export-archive"
if [ ! -f "$ROOT/export-archive/tools/query-result-export/src/lib.rs" ]; then
  echo "exporter archive is missing" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
lock=$(shasum -a 256 "$ROOT/export-archive/tools/query-result-export/Cargo.lock" | awk '{print $1}')
if [ "$lock" != "cc614d3de29ee73fb29ed7bfa9761102f9a3fe26d27244e8e76000016a791d22" ]; then
  echo "archived Cargo.lock mismatch" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a333 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a333 \
  --label oo.kind=network \
  "$NET" >/dev/null

ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}' > "$LIVE/host-cargo-count.txt"
host_n=$(cat "$LIVE/host-cargo-count.txt")
docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$LIVE/builders-before.txt" || true
builder_n=$(grep -c . "$LIVE/builders-before.txt" || true)
printf 'host_cargo=%s rust_builders=%s cap_global=10 cap_own=1\n' "$host_n" "$builder_n" > "$LIVE/capacity.txt"
if [ "$builder_n" -ge 10 ] || [ "$host_n" -ge 1 ]; then
  echo "deferred: rust builder capacity host=$host_n builders=$builder_n" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter status=running --filter label=oo.owner=a333 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$LIVE/docker-run.exit"
  exit 3
fi

docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a333 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a333 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a333 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a333 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null

if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$LIVE/docker-run.exit"
  exit 2
fi
printf '%s\n' "exporter=${EXPORTER} image=pinned-rust-1.94 memory=6g cpus=1 network=${NET} overlay=none" > "$LIVE/invocation.txt"
sync
sleep 2

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/export \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_NET_OFFLINE=true \
  --mount "type=bind,source=${ROOT}/export-archive/tools/query-result-export,target=/src,readonly" \
  --mount "type=bind,source=${LIVE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW}/actual-pg-chain/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a333 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a333 \
  --label oo.task=query-result-export-decimal-actual-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$LIVE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$LIVE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
} > "$LIVE/hostconfig.txt"
docker rm "$NAME" > "$LIVE/container-removed.txt" 2>&1 || true
exit "$run_exit"
