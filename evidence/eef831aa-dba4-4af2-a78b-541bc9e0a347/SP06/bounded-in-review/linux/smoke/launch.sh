#!/bin/bash
# Linux arm64 Rust 1.94 smoke before the review harness. One 6 GiB builder.
# No PostgreSQL. No pull. No prune. Archive is commit 6f87942 only.
# Origin: a343 smoke launch, itself from a200/a274. Change: owner a347, this commit, no overlay.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a347/SP06/bounded-in-review
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a347-eef831aa-dba4-4af2-a78b-541bc9e0a347/smoke
FIXED=6f87942f1c66ba1a9787b7d7194e58a507af63ef
BLOB=6d7aef7bb95c9550a7e6f1bc7f3f22f7c098de4f
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a347-smoke
NET=oo-a347-eef831aa-net
CARGO_VOL=oo-a347-eef831aa-cargo
TARGET_VOL=oo-a347-eef831aa-target

chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh" "$SMOKE/extract-libclang.sh"
if [ -f "$SMOKE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
mkdir -p "$SMOKE/phases" "$ROOT"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates tools/gateway-token-store \
  | tar -x -C "$ROOT/archive"
printf '%s\n' "$FIXED" > "$SMOKE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}:crates/oo-gateway/src/sql_guard.rs" > "$SMOKE/archive-sql-guard-blob.txt"
if [ "$(cat "$SMOKE/archive-sql-guard-blob.txt")" != "$BLOB" ]; then
  echo "archive sql_guard blob is not 6f87942" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/review_bounded_in.rs" ]; then
  echo "archive already contains the review harness" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
{
  for path in \
    crates/oo-gateway/src/sql_guard.rs \
    crates/oo-gateway/tests/bounded_in.rs \
    crates/oo-gateway/src/query_executor.rs \
    crates/oo-gateway/src/policy.rs \
    crates/oo-gateway/src/policy_catalog.rs \
    crates/oo-gateway/src/lib.rs \
    crates/oo-gateway/src/bin/query-pg.rs \
    crates/oo-gateway/Cargo.toml \
    Cargo.lock
  do
    git -C "$SRC" show "${FIXED}:${path}" | shasum -a 256 | awk -v p="$path" '{print $1 "  " p}'
  done
} > "$SMOKE/expected-sha256.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$SMOKE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a347 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a347 \
  --label oo.kind=network \
  "$NET" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a347 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$SMOKE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$SMOKE/docker-run.exit"; exit 1; }
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a347 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a347 \
  --label oo.kind=cargo \
  "$CARGO_VOL" > "$SMOKE/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a347 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a347 \
  --label oo.kind=target \
  "$TARGET_VOL" > "$SMOKE/volume-target.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$SMOKE/invocation.txt"
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
  -e CARGO_TARGET_DIR=/target/a347 \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools/select-debs.rs,target=/tools/select-debs.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a347 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a347 \
  --label oo.task=bounded-in-review-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$SMOKE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$SMOKE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
} > "$SMOKE/hostconfig.txt"
docker rm "$NAME" > "$SMOKE/container-removed.txt"
exit "$run_exit"
