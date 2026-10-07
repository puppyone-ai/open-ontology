#!/bin/bash
# One new 6GiB arm64 builder. Immutable git archive of cd8f754, then the owned overlay.
# Secret values stay in files. This script does not prune and does not remove foreign containers.
set -u
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a341/SP06/gateway-compiled-pack
EVIDENCE=${BASE}/linux/smoke
ROOT=/tmp/oo-a341-eef831aa-dba4-4af2-a78b-541bc9e0a341/proof
FIXTURE=/tmp/oo-a341-eef831aa-dba4-4af2-a78b-541bc9e0a341/builder-fixture
TOOLS=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
COMMIT=cd8f754c1b4db22fc962370c4b29bb846b0fccc3
NAME=oo-a341-proof
NET=oo-a341-eef831aa-net
CARGO_VOL=oo-a341-eef831aa-cargo
TARGET_VOL=oo-a341-eef831aa-target

hash_files() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$@"
  else
    shasum -a 256 "$@"
  fi
}

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh" "$EVIDENCE/extract-libclang.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
mkdir -p "$EVIDENCE"
for secret in token-admin.pass token-runtime.pass ca.crt; do
  if [ ! -s "$FIXTURE/$secret" ]; then
    echo "fixture file missing" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
done
if ! docker exec oo-a341-pg pg_isready -d oo_a341 >/dev/null 2>&1; then
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
if [ ! -d "$ROOT/archive/tools/gateway-token-store" ]; then
  echo "archive is missing the token store" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "$COMMIT" > "$EVIDENCE/archive-commit.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
hash_files "$ROOT/archive/Cargo.lock" "$ROOT/archive/crates/oo-gateway/src/http_gateway.rs" \
  "$ROOT/archive/crates/oo-gateway/src/pack_catalog.rs" > "$EVIDENCE/archive-sha256.txt"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "pinned image is not arm64" >&2; printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"; exit 2; }
docker network inspect "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume create --label oo.project=open-ontology --label oo.owner=a341 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a341 --label oo.kind=cargo "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume create --label oo.project=open-ontology --label oo.owner=a341 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a341 --label oo.kind=target "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
own=$(docker ps -q --filter label=oo.owner=a341 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  -e OO_A341_LIVE=1 \
  -e OO_A341_PG_HOST=oo-a341-pg \
  -e OO_A341_PG_PORT=5432 \
  -e OO_A341_PG_DATABASE=oo_a341 \
  -e OO_A341_ADMIN_PASSWORD_FILE=/fixture/token-admin.pass \
  -e OO_A341_RUNTIME_PASSWORD_FILE=/fixture/token-runtime.pass \
  -e OO_A341_CA_FILE=/fixture/ca.crt \
  -e OO_A341_MARKER=/evidence/live-marker.txt \
  -e OO_A341_FIXTURE_DIR=/overlay/gateway-compiled-pack/fixtures \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${FIXTURE},target=/fixture,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${TOOLS},target=/tools,readonly" \
  --mount "type=bind,source=${SRC}/crates/oo-gateway/src/pack_catalog.rs,target=/overlay/pack_catalog.rs,readonly" \
  --mount "type=bind,source=${SRC}/crates/oo-gateway/tests/http_gateway_pg_live.rs,target=/overlay/http_gateway_pg_live.rs,readonly" \
  --mount "type=bind,source=${SRC}/testing/real-db/gateway-compiled-pack,target=/overlay/gateway-compiled-pack,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a341 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a341 \
  --label oo.task=gateway-compiled-pack \
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
