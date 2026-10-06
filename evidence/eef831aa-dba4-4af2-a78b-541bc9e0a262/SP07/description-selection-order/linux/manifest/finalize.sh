#!/bin/bash
# Rewrite SHA256SUMS after the compile log, exit files, and CLEANUP.txt are closed.
# The only path this script writes inside the review directory is SHA256SUMS.
# Stdout goes to the path in $1 and stays outside the review directory.
# Does not prune. Does not create a volume or a network.
set -u
if [ "$#" -ne 1 ]; then
  echo "usage: finalize.sh <console-outside-review-dir>" >&2
  exit 2
fi
CONSOLE=$1
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a262/SP07/description-selection-order
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a262-manifest-final
case "$CONSOLE" in
  "$REVIEW"/*)
    echo "console path is inside the review directory" >&2
    exit 2
    ;;
esac
if [ ! -f "$REVIEW/linux/manifest/console.log" ]; then
  echo "compile console.log is missing" >&2
  exit 2
fi
if [ ! -f "$REVIEW/linux/manifest/container-removed.txt" ]; then
  echo "compile container removal record is missing" >&2
  exit 2
fi
if [ ! -f "$REVIEW/CLEANUP.txt" ]; then
  echo "CLEANUP.txt is missing" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a262 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$CONSOLE.capacity"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named finalize container already exists" >&2
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
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --label oo.project=open-ontology \
  --label oo.owner=a262 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a262 \
  --label oo.task=description-selection-order-manifest-final \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2021 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/SHA256SUMS' \
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
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$CONSOLE.hostconfig"
docker rm "$NAME" > "$CONSOLE.removed"
exit "$run_exit"
