#!/bin/bash
# One linux/arm64 Rust 1.94 builder. Tests the scorer, then runs dialogue propose.
# Does not start PostgreSQL and does not call the model through pg-builder.
set -u
SRC=/Users/redux/puppy-one/open-ontology
UUID=eef831aa-dba4-4af2-a78b-541bc9e0a389
REVIEW=${SRC}/evidence/${UUID}/SP12/product-benchmark
EVIDENCE=${REVIEW}/linux/propose
CRATE=${SRC}/testing/product-benchmark
PIN=e31635b213911c725159a23dbccc8463cc45cdd7
ROOT=${EVIDENCE}/dialogue-pin
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a389-propose
NET=oo-a389-eef831aa-net
CARGO_VOL=oo-a389-eef831aa-cargo
TARGET_VOL=oo-a389-eef831aa-target
TOKEN=/tmp/oo-customer-demo-20261007/llm-token

mkdir -p "$EVIDENCE/phases"
chmod 755 "$CRATE/linux/propose/inside.sh" "$CRATE/linux/propose/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite propose console.log" >&2
  exit 2
fi
if [ ! -f "$TOKEN" ]; then
  echo "bearer file is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ "$(tr -d '[:space:]' < "$REVIEW/linux/chain/chain.exit")" != "0" ]; then
  echo "chain proof is not exit 0" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
mkdir -p "$ROOT/dialogue-src"
if [ ! -f "$ROOT/dialogue-src/tools/ontology-dialogue/Cargo.toml" ]; then
  git -C "$SRC" archive "$PIN" tools/ontology-dialogue tools/answer-citations | tar -x -C "$ROOT/dialogue-src"
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a389 \
  --label oo.session="$UUID" \
  --label oo.kind=network \
  "$NET" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a389 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named propose container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a389 \
  --label oo.session="$UUID" \
  --label oo.kind=cargo \
  "$CARGO_VOL" > "$EVIDENCE/volume-cargo.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a389 \
  --label oo.session="$UUID" \
  --label oo.kind=target \
  "$TARGET_VOL" > "$EVIDENCE/volume-target.txt"
if [ ! -f "$REVIEW/report/report.before-approved-binding.json" ]; then
  cp "$REVIEW/report/report.json" "$REVIEW/report/report.before-approved-binding.json"
  cp "$REVIEW/report/report.md" "$REVIEW/report/report.before-approved-binding.md"
fi
printf '%s\n' "docker run -d --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
if [ ! -f "$ROOT/dialogue-src/tools/ontology-dialogue/Cargo.toml" ]; then
  echo "dialogue pin archive is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cid=$(docker run -d --name "$NAME" \
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
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${CRATE},target=/crate,readonly" \
  --mount "type=bind,source=${CRATE}/linux/propose/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --mount "type=bind,source=${ROOT}/dialogue-src,target=/dialogue-src,readonly" \
  --mount "type=bind,source=${TOKEN},target=/run/llm-token,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a389 \
  --label oo.session="$UUID" \
  --label oo.task=product-benchmark-propose \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh)
printf '%s\n' "$cid" > "$EVIDENCE/container.id"
if [ -z "$cid" ]; then
  echo "docker run did not return an id" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
echo "propose_started id=$cid"
