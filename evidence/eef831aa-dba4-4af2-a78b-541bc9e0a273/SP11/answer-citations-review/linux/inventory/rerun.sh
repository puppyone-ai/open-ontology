#!/bin/bash
# Rewrite SHA256SUMS after inventory/console.log and cleanup/run.log exist.
# The rerun console stays in /tmp and is not copied into the evidence tree.
# Does not prune. Origin: a266 linux/inventory/rerun.sh with name oo-a273-hash2.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a273/SP11/answer-citations-review
PRODUCT=${SRC}/tools/answer-citations
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a273-hash2
CONSOLE=/tmp/oo-a273-eef831aa-dba4-4af2-a78b-541bc9e0a273/hash2-console.log
ROOT=/tmp/oo-a273-eef831aa-dba4-4af2-a78b-541bc9e0a273

mkdir -p "$ROOT"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a273 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named hash2 container already exists" >&2
  exit 2
fi
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
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=bind,source=${EVIDENCE}/tools/sha256.rs,target=/tool/sha256.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${PRODUCT},target=/product,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a273 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a273 \
  --label oo.task=answer-citations-review-inventory-rerun \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'rustc --edition 2021 -C debuginfo=0 -o /sha256 /tool/sha256.rs && /sha256 dir /evidence/SHA256SUMS /evidence && /sha256 dir /evidence/product-SHA256SUMS /product' \
  > "$CONSOLE" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$ROOT/hash2.exit"
docker rm "$NAME" > "$ROOT/hash2-removed.txt"
exit "$run_exit"
