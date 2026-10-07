#!/bin/bash
# Final sums. Writes only the two SHA256SUMS files. Stdout and hostconfig
# stay outside the hashed trees. Does not prune.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh
# sha256 c59441884fcd7ce54b8e6cb11a6c632cf5dc8ba757d112145fbf20ede8d83ac3
# Changes: UUID a318, 6GiB, 1 CPU, global cap 10, copy-then-walk, two roots.
set -u
if [ "$#" -ne 2 ]; then
  echo "usage: final-launch.sh <console-outside> <hostconfig-outside>" >&2
  exit 2
fi
CONSOLE=$1
HOSTCONFIG=$2
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a318
TOOL=${SRC}/tools/local-pg-mirror
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a318-sha256
case "$CONSOLE" in
  "$REVIEW"/*|"$TOOL"/*)
    echo "console path is inside a hashed tree" >&2
    exit 2
    ;;
esac
case "$HOSTCONFIG" in
  "$REVIEW"/*|"$TOOL"/*)
    echo "hostconfig path is inside a hashed tree" >&2
    exit 2
    ;;
esac
if [ -e "$REVIEW/SHA256SUMS" ] || [ -e "$TOOL/SHA256SUMS" ]; then
  echo "SHA256SUMS already exists" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a318 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity own=$own global=$global_builders host_cargo=$host_cargo" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named inventory container already exists" >&2
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
  --mount "type=bind,source=${TOOL},target=/tool" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --label oo.project=open-ontology \
  --label oo.owner=a318 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a318 \
  --label oo.task=local-pg-mirror-inventory-final \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'set -eu
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
copy_ok() {
  src=$1
  dst=$2
  n=0
  while [ "$n" -lt 5 ]; do
    rm -rf "$dst"
    mkdir -p "$dst"
    if cp -a "$src"/. "$dst"/; then
      return 0
    fi
    n=$((n+1))
    echo "copy_retry=$n src=$src"
    sleep 1
  done
  return 1
}
echo copy_evidence
copy_ok /evidence /work/evidence
echo copy_tool
copy_ok /tool /work/tool
if [ -e /work/evidence/SHA256SUMS ] || [ -e /work/tool/SHA256SUMS ]; then
  echo "copy already contains SHA256SUMS" >&2
  exit 2
fi
echo compile
rustc --edition 2024 -D warnings -o /work/sha256_manifest /work/evidence/SRC/local-pg-mirror/tools/sha256_manifest.rs
echo hash_evidence
/work/sha256_manifest /work/evidence /evidence/SHA256SUMS
echo hash_tool
/work/sha256_manifest /work/tool /tool/SHA256SUMS
echo inventory_ok
' > "$CONSOLE" 2>&1
run_exit=$?
set +e
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$HOSTCONFIG"
docker rm "$NAME" >/dev/null 2>&1
exit "$run_exit"
