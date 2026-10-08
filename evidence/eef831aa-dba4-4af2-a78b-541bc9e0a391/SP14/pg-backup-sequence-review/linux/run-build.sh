#!/bin/bash
# One Rust 1.94 builder. Does not pull, prune, or use the host compiler.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a391
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/pg-backup-sequence-review
ROOT=${EVIDENCE}/runtime
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a391-eef831aa-net
BNAME=oo-a391-build
CARGO_VOL=oo-a391-eef831aa-cargo
TARGET_VOL=oo-a391-eef831aa-target
PRODUCT=95c33fca0b22b38a2e08c08bb5e2a050f0bf44ec

mkdir -p "$EVIDENCE/linux/build/phases" "$ROOT/out"
chmod 755 "$EVIDENCE/linux/inside.sh" "$EVIDENCE/linux/run-build.sh"
if [ ! -f "$EVIDENCE/linux/pre-edit-smoke/inside.exit" ] || [ "$(cat "$EVIDENCE/linux/pre-edit-smoke/inside.exit")" != 0 ]; then
  echo "pre-edit smoke is missing" >&2
  exit 2
fi
if [ -f "$EVIDENCE/linux/build/inside.exit" ]; then
  echo "build evidence already exists" >&2
  exit 2
fi
if ! docker image inspect "$RUST_IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/linux/build/docker-run.exit"
  exit 1
fi
docker image inspect "$RUST_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/linux/build/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/linux/build/image.txt"
own=$(docker ps -q --filter label=oo.owner=a391 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/linux/build/capacity.txt"
printf '%s\n' "$global_builders" > "$EVIDENCE/linux/build/builders-before.txt"
printf '%s\n' "$host_cargo" > "$EVIDENCE/linux/build/host-cargo.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/build/docker-run.exit"
  exit 3
fi
if docker container inspect "$BNAME" >/dev/null 2>&1; then
  echo "named builder already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/build/docker-run.exit"
  exit 2
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a391 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" >/dev/null
fi
ensure_volume() {
  vol=$1
  kind=$2
  if docker volume inspect "$vol" >/dev/null 2>&1; then
    owner=$(docker volume inspect --format '{{index .Labels "oo.owner"}}' "$vol")
    session=$(docker volume inspect --format '{{index .Labels "oo.session"}}' "$vol")
    if [ "$owner" != "a391" ] || [ "$session" != "$RUN_ID" ]; then
      echo "volume ${vol} is not owned by this session" >&2
      exit 2
    fi
    printf '%s\n' "reused ${vol}"
  else
    docker volume create --label oo.project=open-ontology --label oo.owner=a391 --label oo.session="${RUN_ID}" --label oo.kind="${kind}" "$vol"
  fi
}
ensure_volume "$CARGO_VOL" rust-builder > "$EVIDENCE/linux/build/volume-cargo.txt"
ensure_volume "$TARGET_VOL" rust-builder > "$EVIDENCE/linux/build/volume-target.txt"
printf '%s\n' "$PRODUCT" > "$EVIDENCE/linux/build/fixed-commit.txt"
printf '%s\n' "docker run --name ${BNAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/linux/build/invocation.txt"

set +e
docker run --name "$BNAME" \
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
  -e CARGO_TARGET_DIR=/target/review \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${REPO}/tools/pg-backup-restore,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/linux/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${ROOT}/out,target=/out" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a391 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=pg-backup-sequence-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/linux/build/console.log" 2>&1
build_code=$?
set -e
printf '%s\n' "$build_code" > "$EVIDENCE/linux/build/docker-run.exit"
docker inspect "$BNAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$EVIDENCE/linux/build/hostconfig.txt" || true
docker inspect "$BNAME" --format '{{json .Mounts}}' > "$EVIDENCE/linux/build/mounts.txt" || true
docker rm -f "$BNAME" >/dev/null
printf '%s\n' removed > "$EVIDENCE/linux/build/container-removed.txt"
if [ "$build_code" -ne 0 ]; then
  echo "builder exit ${build_code}" >&2
  exit "$build_code"
fi
if [ ! -x "$ROOT/out/pg-backup-restore" ]; then
  echo "release binary is missing" >&2
  exit 1
fi
echo "build finished"
