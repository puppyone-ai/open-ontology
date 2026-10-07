#!/bin/bash
# Staging inventory. Copies both trees into the container writable layer and
# compiles the unchanged a199 tool. Sums from this process stay on the
# container tmpfs. Stdout goes to $1, outside the hashed trees.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh
# sha256 c59441884fcd7ce54b8e6cb11a6c632cf5dc8ba757d112145fbf20ede8d83ac3
# Changes: UUID a318, 6GiB, 1 CPU, global cap 10, copy-then-walk, no sums file.
set -u
if [ "$#" -ne 1 ]; then
  echo "usage: launch.sh <console-outside-hashed-trees>" >&2
  exit 2
fi
CONSOLE=$1
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a318
TOOL=${SRC}/tools/local-pg-mirror
EVIDENCE=${REVIEW}/SRC/local-pg-mirror/linux/inventory
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a318-sha256
case "$CONSOLE" in
  "$REVIEW"/*|"$TOOL"/*)
    echo "console path is inside a hashed tree" >&2
    exit 2
    ;;
esac
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a318 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named inventory container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 copy-then-compile" > "$EVIDENCE/invocation.txt"
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
  --mount "type=bind,source=${REVIEW},target=/evidence,readonly" \
  --mount "type=bind,source=${TOOL},target=/tool,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --label oo.project=open-ontology \
  --label oo.owner=a318 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a318 \
  --label oo.task=local-pg-mirror-inventory-staging \
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
mkdir -p /work/tmp
echo copy_evidence
copy_ok /evidence /work/evidence
echo copy_tool
copy_ok /tool /work/tool
echo compile
rustc --edition 2024 -D warnings -o /work/sha256_manifest /work/evidence/SRC/local-pg-mirror/tools/sha256_manifest.rs
echo hash_staging_evidence
/work/sha256_manifest /work/evidence /work/tmp/evidence.sha256
echo hash_staging_tool
/work/sha256_manifest /work/tool /work/tmp/tool.sha256
echo inventory_staging_ok
' > "$CONSOLE" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
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
} > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt" 2>&1
exit "$run_exit"
