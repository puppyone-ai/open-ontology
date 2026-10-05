#!/bin/bash
# One proof builder. Requires a successful pre-overlay smoke and the running fixture.
# Copies the review overlay onto the immutable archive. Does not move git source.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a191/SP06/query-executor-shutdown-review
SMOKE=${EVIDENCE}/linux/smoke
PROOF=${EVIDENCE}/linux/proof
ROOT=/tmp/oo-a191-eef831aa-dba4-4af2-a78b-541bc9e0a191
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a191-proof
NET=oo-a191-net
CARGO_VOL=oo-a191-cargo
TARGET_VOL=oo-a191-target
SECRETS=${ROOT}/secrets/client
OVERLAY=${EVIDENCE}/overlay/query_executor_a191_review.rs
DEST=${ROOT}/archive/crates/oo-gateway/tests/query_executor_a191_review.rs

chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh" "$SMOKE/extract-libclang.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
if [ ! -f "$SMOKE/smoke.exit" ]; then
  echo "refusing proof before the pre-overlay smoke records an exit" >&2
  exit 2
fi
smoke_code=$(tr -d '[:space:]' < "$SMOKE/smoke.exit")
printf '%s\n' "$smoke_code" > "$PROOF/smoke-exit-seen.txt"
if [ "$smoke_code" != "0" ]; then
  echo "refusing proof before a successful pre-overlay smoke" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/query_executor.rs" ]; then
  echo "fixed archive is missing" >&2
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/src/parser_worker.rs" ]; then
  echo "archive contains parser_worker.rs" >&2
  exit 2
fi
if [ ! -s "$OVERLAY" ]; then
  echo "overlay source is missing" >&2
  exit 2
fi
mkdir -p "$(dirname "$DEST")" "$PROOF/phases" "$EVIDENCE/live"
cp "$OVERLAY" "$DEST"
chmod 644 "$DEST"
if [ ! -s "$SECRETS/admin.pass" ] || [ ! -s "$SECRETS/login.pass" ] || [ ! -s "$SECRETS/ca.crt" ]; then
  echo "client secret files are missing" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "private network is missing" >&2
  exit 2
fi
pg=$(docker ps -q --filter name=^/oo-a191-pg$ --filter status=running)
if [ -z "$pg" ]; then
  echo "fixture postgres is not running" >&2
  exit 2
fi
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PROOF/host-cargo-procs.txt" || true
host_n=$(grep -c . "$PROOF/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --filter status=running --format '{{.Names}}' > "$PROOF/builders-before.txt" || true
builder_n=$(grep -c . "$PROOF/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=3\n' "$host_n" "$builder_n" "$global_n" > "$PROOF/capacity.txt"
if [ "$global_n" -ge 3 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a191 --filter label=oo.kind=rust-builder --filter status=running)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$PROOF/image.txt"
grep -q 'arch=arm64' "$PROOF/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL image=pinned-rust-1.94 bash /inside.sh env=OO_A191_LIVE,OO_A191_HOST,OO_A191_PORT,OO_A191_ADMIN_PASS_FILE,OO_A191_LOGIN_PASS_FILE,OO_A191_CA,OO_A191_ORACLE,OO_A191_PROXY_HOST,OO_A191_PROXY_PORT secrets=readonly-files" > "$PROOF/invocation.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
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
  -e OO_A191_LIVE=1 \
  -e OO_A191_HOST=oo-a191-pg \
  -e OO_A191_PORT=5432 \
  -e OO_A191_ADMIN_PASS_FILE=/secrets/admin.pass \
  -e OO_A191_LOGIN_PASS_FILE=/secrets/login.pass \
  -e OO_A191_CA=/secrets/ca.crt \
  -e OO_A191_ORACLE=/evidence/live/oracle.txt \
  -e OO_A191_PROXY_HOST=oo-a191-proxy \
  -e OO_A191_PROXY_PORT=5432 \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools/leakscan.rs,target=/tools/leakscan.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SECRETS},target=/secrets,readonly" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a191 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a191 \
  --label oo.task=query-executor-shutdown-review-proof \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROOF/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROOF/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$PROOF/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$PROOF/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$PROOF/container-removed.txt" 2>/dev/null || true
printf '%s\n' "$run_exit" > "$PROOF/proof.exit"
exit "$run_exit"
