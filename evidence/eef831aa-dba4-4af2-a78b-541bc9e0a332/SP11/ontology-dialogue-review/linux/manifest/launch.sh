#!/bin/bash
# Seal MANIFEST.sha256 with the std-only tool in the pinned 1.94 image.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/manifest/launch.sh
# Changes: owner a332, memory 6g, cpus 1, global cap 10, network none.
# The first stdout is copied into this directory. The second stdout is $1 and is not copied back.
set -u
if [ "$#" -ne 1 ]; then
  echo "usage: launch.sh <refresh-stdout-outside-review>" >&2
  exit 2
fi
REFRESH=$1
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a332/SP11/ontology-dialogue-review
EVIDENCE=${REVIEW}/linux/manifest
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a332-manifest
NAME2=oo-a332-manifest-refresh
FIRST=/tmp/oo-a332-manifest-first.stdout
case "$REFRESH" in
  "$REVIEW"/*)
    echo "refresh stdout is inside the review directory" >&2
    exit 2
    ;;
esac
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite manifest console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
mkdir -p "$EVIDENCE"
capacity() {
  own=$(docker ps -q --filter label=oo.owner=a332 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
  printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo"
  if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
    return 3
  fi
  return 0
}
if ! line=$(capacity); then
  printf '%s\n' "$line" > "$EVIDENCE/capacity.txt"
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
printf '%s\n' "$line" > "$EVIDENCE/capacity.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named manifest container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc tools/sha256_manifest.rs" > "$EVIDENCE/invocation.txt"

run_one() {
  local name="$1"
  local console="$2"
  local script="$3"
  docker run --name "$name" \
    --platform linux/arm64 \
    --pull never \
    --init \
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
    --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
    --label oo.project=open-ontology \
    --label oo.owner=a332 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a332 \
    --label oo.task=ontology-dialogue-review-manifest \
    --label oo.toolchain=1.94.0 \
    --label oo.platform=linux/arm64 \
    --label oo.kind=rust-builder \
    "$IMAGE" \
    bash -c "$script" > "$console" 2>&1
}

set +e
run_one "$NAME" "$FIRST" 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256'
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
cp "$FIRST" "$EVIDENCE/console.log"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
if [ "$run_exit" -ne 0 ]; then
  echo "first manifest container failed" >&2
  exit "$run_exit"
fi
if ! line2=$(capacity); then
  echo "deferred before refresh: ${line2}" >&2
  exit 3
fi
echo "refresh_capacity ${line2}"
if docker container inspect "$NAME2" >/dev/null 2>&1; then
  echo "named refresh container already exists" >&2
  exit 2
fi
set +e
run_one "$NAME2" "$REFRESH" 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && rustc --edition 2024 -D warnings /evidence/tools/manifest_check.rs -o /usr/local/bin/manifest_check && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256 && /usr/local/bin/manifest_check manifest /evidence/MANIFEST.sha256 /evidence'
refresh_exit=$?
set +e
echo "refresh_ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME2")"
echo "refresh_OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME2")"
echo "refresh_Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME2")"
echo "refresh_NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME2")"
echo "refresh_CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME2")"
echo "refresh_Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME2")"
echo "refresh_NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME2")"
echo "refresh_PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME2")"
echo "refresh_Id=$(docker inspect -f '{{.Id}}' "$NAME2")"
docker rm "$NAME2"
if [ "$refresh_exit" -ne 0 ]; then
  echo "refresh manifest container failed" >&2
  exit "$refresh_exit"
fi
files=$(awk -F= '$1=="files"{print $2}' "$REFRESH" | head -1)
matched=$(awk -F= '$1=="match"{print $2}' "$REFRESH" | head -1)
mismatched=$(awk -F= '$1=="mismatch"{print $2}' "$REFRESH" | head -1)
missing=$(awk -F= '$1=="missing"{print $2}' "$REFRESH" | head -1)
actual=$(find "$REVIEW" -type f | wc -l | tr -d ' ')
links=$(find "$REVIEW" -type l | wc -l | tr -d ' ')
echo "sealed_files=${files} match=${matched} mismatch=${mismatched} missing=${missing} tree_files=${actual} symlinks=${links}"
if [ "$links" -ne 0 ] || [ "$mismatched" != 0 ] || [ "$missing" != 0 ] || [ "$matched" != "$files" ]; then
  echo "manifest check did not match" >&2
  exit 1
fi
expected=$((files + 1))
if [ "$actual" -ne "$expected" ]; then
  echo "tree file count ${actual} != manifest files+1 ${expected}" >&2
  exit 1
fi
exit 0
