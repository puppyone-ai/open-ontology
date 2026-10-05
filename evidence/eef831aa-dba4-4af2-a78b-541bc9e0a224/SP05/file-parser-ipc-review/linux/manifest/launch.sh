#!/bin/bash
# Compile and run the std-only manifest tool. The first stdout path must sit
# outside this review directory. A second container rewrites MANIFEST.sha256
# after console.log and hostconfig.txt are closed. The second stdout stays
# outside this directory. Does not prune. Does not pull.
set -u
if [ "$#" -ne 2 ]; then
  echo "usage: launch.sh <first-console-outside> <second-console-outside>" >&2
  exit 2
fi
CONSOLE=$1
SECOND=$2
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a224/SP05/file-parser-ipc-review
EVIDENCE=${REVIEW}/linux/manifest
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a224-manifest
REWRITE=oo-a224-manifest-rewrite
case "$CONSOLE" in
  "$REVIEW"/*) echo "first console path is inside the review directory" >&2; exit 2 ;;
esac
case "$SECOND" in
  "$REVIEW"/*) echo "second console path is inside the review directory" >&2; exit 2 ;;
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
chmod 755 "$EVIDENCE/launch.sh"
own=$(docker ps -q --filter label=oo.owner=a224 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1 || docker container inspect "$REWRITE" >/dev/null 2>&1; then
  echo "named manifest container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
if ! grep -q 'arch=arm64' "$EVIDENCE/image.txt"; then
  echo "pinned image is not arm64" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc -o /usr/local/bin/sha256_manifest tools/sha256_manifest.rs" > "$EVIDENCE/invocation.txt"
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
  --label oo.owner=a224 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a224 \
  --label oo.task=file-parser-ipc-review-manifest \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256' \
  > "$CONSOLE" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
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
} > "$EVIDENCE/hostconfig.txt"
cp "$CONSOLE" "$EVIDENCE/console.log"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
if [ "$run_exit" -ne 0 ]; then
  exit "$run_exit"
fi
docker run --name "$REWRITE" \
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
  --label oo.owner=a224 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a224 \
  --label oo.task=file-parser-ipc-review-manifest-rewrite \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256 && /usr/local/bin/sha256_manifest /evidence /tmp/again.sha256 && cmp /evidence/MANIFEST.sha256 /tmp/again.sha256' \
  > "$SECOND" 2>&1
rewrite_exit=$?
{
  echo "rewrite_exit=$rewrite_exit"
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$REWRITE")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$REWRITE")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$REWRITE")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$REWRITE")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$REWRITE")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$REWRITE")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$REWRITE")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$REWRITE")"
} >> "$SECOND"
docker rm "$REWRITE" >> "$SECOND"
exit "$rewrite_exit"
