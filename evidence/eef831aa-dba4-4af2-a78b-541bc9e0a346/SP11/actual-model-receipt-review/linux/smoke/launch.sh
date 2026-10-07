#!/bin/bash
# Linux arm64 Rust 1.94 smoke before any a346 review automation.
# Origin: a332 preoverlay launch. Archive 63566b60. No PostgreSQL. No model call.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a346/SP11/actual-model-receipt-review
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/var/tmp/oo-a346-eef831aa-dba4-4af2-a78b-541bc9e0a346/smoke
FIXED=63566b60aabdf29a29bfe55c4866edf3c4081cfd
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a346-smoke
NET=oo-a346-eef831aa-net
CARGO_VOL=oo-a346-eef831aa-cargo
TARGET_VOL=oo-a346-eef831aa-target
A332=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a332/SP11/ontology-dialogue-review/linux/preoverlay/launch.sh
A332_IN=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a332/SP11/ontology-dialogue-review/linux/preoverlay/inside.sh

mkdir -p "$SMOKE/phases" "$ROOT"
chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh"
if [ -f "$SMOKE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$SMOKE/docker-run.exit"
  exit 1
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt" || { echo "rust image is not arm64" >&2; printf '%s\n' 1 > "$SMOKE/docker-run.exit"; exit 1; }
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a346 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a346 \
    --label oo.kind=network \
    "$NET" >/dev/null
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi

own_docker=$(docker ps -q --filter label=oo.owner=a346 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_docker=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_host=0
global_host=0
while read -r pid comm; do
  [ -n "${pid:-}" ] || continue
  global_host=$((global_host + 1))
  cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | awk '/^n/ {print substr($0,2); exit}')
  case "$cwd" in
    *oo-a346*) own_host=$((own_host + 1)) ;;
  esac
done <<EOF
$(ps -axo pid=,comm= | awk '$2=="cargo" || $2=="rustc" || $2=="rustdoc" || $2=="rustup" {print $1, $2}')
EOF
own=$((own_docker + own_host))
global=$((global_docker + global_host))
printf 'own=%s global=%s own_docker=%s own_host=%s global_docker=%s global_host=%s cap_global=10 cap_own=1\n' \
  "$own" "$global" "$own_docker" "$own_host" "$global_docker" "$global_host" > "$SMOKE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 10 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi

{
  echo "a332 preoverlay launch ${A332} $(shasum -a 256 "$A332" | awk '{print $1}')"
  echo "a332 preoverlay inside ${A332_IN} $(shasum -a 256 "$A332_IN" | awk '{print $1}')"
  echo "fixed=${FIXED}"
  echo "changes owner a346 private volumes network memory=6g cpus=1 pids=4096 no-tmpfs no-pg no-live no-model"
  echo "attempt-1 kept the default docker tmpfs noexec on /tmp; the approval tests spawn a shell there and returned io"
} > "$SMOKE/recipe-origins.txt"

ARCHIVE=${ROOT}/archive
rm -rf "$ARCHIVE"
mkdir -p "$ARCHIVE"
git -C "$SRC" archive "$FIXED" | tar -x -C "$ARCHIVE"
test -f "$ARCHIVE/tools/ontology-dialogue/src/lib.rs"
rm -f "$ARCHIVE/rust-toolchain.toml"
printf '%s\n' "omitted from the private archive copy; image toolchain is 1.94.0" > "$SMOKE/toolchain-omitted.txt"
printf '%s\n' "$FIXED" > "$SMOKE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$SMOKE/fixed-tree.txt"
git -C "$SRC" rev-parse "${FIXED}:tools/ontology-dialogue" > "$SMOKE/dialogue-tree.txt"
grep -q '^1941b8a6a885ec85ac93da6cc6f4d45c8ecb48a0$' "$SMOKE/dialogue-tree.txt"

if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology --label oo.owner=a346 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a346 --label oo.kind=cargo \
    "$CARGO_VOL" > "$SMOKE/volume-cargo.txt"
else
  docker volume inspect -f '{{.Name}}' "$CARGO_VOL" > "$SMOKE/volume-cargo.txt"
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology --label oo.owner=a346 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a346 --label oo.kind=target \
    "$TARGET_VOL" > "$SMOKE/volume-target.txt"
else
  docker volume inspect -f '{{.Name}}' "$TARGET_VOL" > "$SMOKE/volume-target.txt"
fi

printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94" > "$SMOKE/invocation.txt"
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
  --mount "type=bind,source=${ARCHIVE},target=/src,readonly" \
  --mount "type=bind,source=${SMOKE},target=/evidence/linux/smoke" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a346 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a346 \
  --label oo.task=actual-model-receipt-review-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /evidence/linux/smoke/inside.sh > "$SMOKE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$SMOKE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME" 2>/dev/null || echo missing)"
} > "$SMOKE/hostconfig.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  docker rm "$NAME" > "$SMOKE/container-removed.txt"
fi
exit "$run_exit"
