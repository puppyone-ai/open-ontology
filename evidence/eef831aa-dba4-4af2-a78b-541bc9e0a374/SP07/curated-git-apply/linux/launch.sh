#!/bin/bash
# Hash source and evidence. One container at a time. No pull. No product mount.
set -euo pipefail
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a374/SP07/curated-git-apply
TMP=/tmp/cga-hash-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
COMMIT=576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6

rm -rf "$TMP"
mkdir -p "$TMP/src" "$TMP/log"
GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_OPTIONAL_LOCKS=0 \
  git -C "$REPO" archive --format=tar "$COMMIT" tools/curated-git-apply | tar -x -C "$TMP/src"
find "$TMP/src" -type l -print | grep -q . && { echo "symlink in source extract" >&2; exit 1; }
files=$(find "$TMP/src" -type f | wc -l | tr -d ' ')
printf '%s\n' "$files" > "$EVIDENCE/linux/hash/source-extract-count.txt"
if [ "$files" != 19 ]; then
  echo "source extract file count is $files" >&2
  exit 1
fi

own=$(docker ps -q --filter label=oo.owner=a374 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/linux/hash/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  exit 3
fi

run_one() {
  local name=$1
  local mode=$2
  if docker container inspect "$name" >/dev/null 2>&1; then
    echo "named hash container already exists" >&2
    exit 2
  fi
  set +e
  docker run --name "$name" \
    --platform linux/arm64 \
    --pull never \
    --user 0:0 \
    --security-opt no-new-privileges \
    --cap-drop ALL \
    --memory 6g \
    --memory-swap 6g \
    --cpus 1 \
    --pids-limit 512 \
    --network none \
    -e HOME=/tmp \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    --mount "type=bind,source=${EVIDENCE},target=/evidence" \
    --mount "type=bind,source=${TMP}/src,target=/src" \
    --label oo.project=open-ontology \
    --label oo.owner=a374 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a374 \
    --label oo.kind=rust-builder \
    --label oo.task=curated-git-apply-hash \
    --label oo.platform=linux/arm64 \
    "$IMAGE" \
    bash /evidence/linux/hash-inside.sh "$mode" \
    > "$TMP/log/${mode}.console" 2>&1
  local code=$?
  set -e
  printf '%s\n' "$code" > "$TMP/log/${mode}.exit"
  docker inspect "$name" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}} capdrop={{json .HostConfig.CapDrop}}' > "$TMP/log/${mode}.hostconfig" || true
  docker rm -f "$name" >/dev/null
  printf '%s\n' removed > "$TMP/log/${mode}.removed"
  return "$code"
}

run_one cga-eef831aa-hash source
cp "$TMP/log/source.console" "$EVIDENCE/linux/hash/console.log"
cp "$TMP/log/source.exit" "$EVIDENCE/linux/hash/docker-run.exit"
cp "$TMP/log/source.hostconfig" "$EVIDENCE/linux/hash/hostconfig.txt"
cp "$TMP/log/source.removed" "$EVIDENCE/linux/hash/container-removed.txt"
{
  printf '%s linux/hash/docker-run.exit\n' "$(cat "$EVIDENCE/linux/hash/docker-run.exit")"
  printf '%s linux/hash/rustc.exit\n' "$(cat "$EVIDENCE/linux/hash/rustc.exit")"
  printf '%s linux/hash/source-manifest.exit\n' "$(cat "$EVIDENCE/linux/hash/source-manifest.exit")"
} > "$EVIDENCE/raw/hash-exits.txt"

set +e
run_one cga-eef831aa-hash2 final
final_code=$?
set -e
if [ "$final_code" != 0 ]; then
  mkdir -p /tmp/cga-hash-eef831aa-failed
  cp "$TMP/log/final.console" "$TMP/log/final.exit" /tmp/cga-hash-eef831aa-failed/ 2>/dev/null || true
  echo "final hash exit $final_code" >&2
  exit "$final_code"
fi
rm -rf "$TMP"
