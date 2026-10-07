#!/bin/bash
# One 6GiB arm64 builder. Immutable git archive of the implementation commit.
# No product overlay. Secret values stay in files. This script does not prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http
HARNESS=${BASE}/linux/proof
EVIDENCE=${BASE}/linux/proof
ROOT=/tmp/oo-a326-eef831aa-dba4-4af2-a78b-541bc9e0a326/proof
FIXTURE=/tmp/oo-a326-eef831aa-dba4-4af2-a78b-541bc9e0a326/builder-fixture
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
COMMIT=0a5c77af1165a4a9fcb2be7f87ecf8c33c891035
NAME=oo-a326-proof
NET=oo-a326-eef831aa-net
CARGO_VOL=oo-a326-eef831aa-cargo
TARGET_VOL=oo-a326-eef831aa-target

chmod 755 "$HARNESS/inside.sh" "$HARNESS/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE"
if [ ! -f "$EVIDENCE/Cargo.lock.pre-consumer" ]; then
  echo "pre-consumer lock is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
for secret in token-admin.pass token-runtime.pass ca.crt; do
  if [ ! -s "$FIXTURE/$secret" ]; then
    echo "fixture file missing" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
done
if ! docker exec oo-a326-pg pg_isready -d oo_a326 >/dev/null 2>&1; then
  echo "private postgres is not accepting connections" >&2
  printf '%s\n' 4 > "$EVIDENCE/docker-run.exit"
  exit 4
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$COMMIT" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates tools/gateway-token-store \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/http_gateway.rs" ]; then
  echo "archive is missing the gateway" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "$COMMIT" > "$EVIDENCE/archive-commit.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
sha256sum "$ROOT/archive/Cargo.lock" "$ROOT/archive/crates/oo-gateway/src/http_gateway.rs" > "$EVIDENCE/archive-sha256.txt"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "pinned image is not arm64" >&2; printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"; exit 2; }
docker network inspect "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
own=$(docker ps -q --filter label=oo.owner=a326 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 archive=${COMMIT} bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A326_LIVE=1 \
  -e OO_A326_PG_HOST=oo-a326-pg \
  -e OO_A326_PG_PORT=5432 \
  -e OO_A326_PG_DATABASE=oo_a326 \
  -e OO_A326_ADMIN_PASSWORD_FILE=/fixture/token-admin.pass \
  -e OO_A326_RUNTIME_PASSWORD_FILE=/fixture/token-runtime.pass \
  -e OO_A326_CA_FILE=/fixture/ca.crt \
  -e OO_A326_MARKER=/evidence/live-marker.txt \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${FIXTURE},target=/fixture,readonly" \
  --mount "type=bind,source=${HARNESS}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${BASE}/linux/smoke-2/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${BASE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a326 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a326 \
  --label oo.task=gateway-http \
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
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
