#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke. One 6 GiB builder, no PostgreSQL, no pull, no prune.
# Origin: a200 smoke launch 837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163
# and a274 focused launch e9fd0027356f5fc90ebe56c0070f1078ba9f0f8d6468fbbecd4db98b4ff16910.
# Change: owner a343, archive 21cc962 path list, existing sql_guard tests, no overlay.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a343/SP06/bounded-in/linux/smoke
ROOT=/tmp/oo-a343-eef831aa-dba4-4af2-a78b-541bc9e0a343/smoke
FIXED=21cc96241a841b47dbbcb116549030e38a643ba0
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a343-smoke
NET=oo-a343-eef831aa-net
CARGO_VOL=oo-a343-eef831aa-cargo
TARGET_VOL=oo-a343-eef831aa-target

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh" "$EVIDENCE/extract-libclang.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/phases" "$ROOT"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates tools/gateway-token-store \
  | tar -x -C "$ROOT/archive"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}:crates/oo-gateway/src/sql_guard.rs" > "$EVIDENCE/archive-sql-guard-blob.txt"
if [ "$(cat "$EVIDENCE/archive-sql-guard-blob.txt")" != "1db8f3ed11a2d816d485bdd2865f37aff3e5cbed" ]; then
  echo "archive sql_guard blob is not the a317 numeric-approved copy" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/bounded_in.rs" ]; then
  echo "archive already contains bounded_in.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "overlay=absent" > "$EVIDENCE/overlay-absent.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a343 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a343 \
  --label oo.kind=network \
  "$NET" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a343 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a343 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a343 \
  --label oo.kind=cargo \
  "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a343 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a343 \
  --label oo.kind=target \
  "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  -e CARGO_TARGET_DIR=/target/a343-smoke \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a343/SP06/bounded-in/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a343/SP06/bounded-in/tools/select-debs.rs,target=/tools/select-debs.rs,readonly" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a343/SP06/bounded-in,target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a343 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a343 \
  --label oo.task=bounded-in-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
