#!/bin/bash
# Rewrite the two manifests after launch.sh has closed its console and inspect files.
# Stdout stays outside the evidence directory. Does not prune.
# Origin: a225 inventory rerun, owner a250. The second stdout is not copied here.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a250/SP12/trial-report-adapter
CRATE=${SRC}/benchmarks/trial-report-adapter
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a250-hash-2
NET=oo-a250-eef831aa-net
ROOT=/tmp/oo-a250-eef831aa-dba4-4af2-a78b-541bc9e0a250
CONSOLE=${ROOT}/hash-again.log

mkdir -p "$ROOT"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
docker network inspect "$NET" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a250 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named hash-2 container already exists" >&2
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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=bind,source=${EVIDENCE}/tools/sha256_manifest.rs,target=/tool/sha256_manifest.rs,readonly" \
  --mount "type=bind,source=${CRATE},target=/crate,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --label oo.project=open-ontology \
  --label oo.owner=a250 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a250 \
  --label oo.task=trial-report-adapter-inventory-rerun \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'rustc --edition 2021 -C debuginfo=0 -o /sha256_manifest /tool/sha256_manifest.rs && /sha256_manifest /crate /evidence/crate-sha256.txt && /sha256_manifest /evidence /evidence/SHA256SUMS.txt' \
  > "$CONSOLE" 2>&1
run_exit=$?
set +e
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$ROOT/hash-again-hostconfig.txt"
docker rm "$NAME" > "$ROOT/hash-again-container-removed.txt"
printf '%s\n' "$run_exit" > "$ROOT/hash-again.exit"
exit "$run_exit"
