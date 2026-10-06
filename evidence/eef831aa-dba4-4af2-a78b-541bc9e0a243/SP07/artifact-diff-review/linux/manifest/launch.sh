#!/bin/bash
# Compile and run the a199 std-only manifest tool. Stdout goes to the path in $1,
# which must sit outside this review directory. Does not prune.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs
# sha256 d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3
# Changes: UUID a243, 6GiB, 1 CPU, global cap 10, private network none.
set -u
if [ "$#" -ne 1 ]; then
  echo "usage: launch.sh <console-outside-review-dir>" >&2
  exit 2
fi
CONSOLE=$1
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a243/SP07/artifact-diff-review
EVIDENCE=${REVIEW}/linux/manifest
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a243-manifest
case "$CONSOLE" in
  "$REVIEW"/*)
    echo "console path is inside the review directory" >&2
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
own=$(docker ps -q --filter label=oo.owner=a243 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named manifest container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
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
  --label oo.owner=a243 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a243 \
  --label oo.task=artifact-diff-review-manifest \
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
# Second hash sees hostconfig, console.log, and the exit files.
# Its stdout stays outside this review directory and is not copied in.
PASS2_NAME=oo-a243-manifest-pass2
PASS2_CONSOLE="${CONSOLE}.pass2"
if docker container inspect "$PASS2_NAME" >/dev/null 2>&1; then
  echo "named manifest pass2 container already exists" >&2
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a243 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  exit 3
fi
set +e
docker run --name "$PASS2_NAME" \
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
  --label oo.owner=a243 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a243 \
  --label oo.task=artifact-diff-review-manifest-pass2 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256' \
  > "$PASS2_CONSOLE" 2>&1
pass2_exit=$?
set +e
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$PASS2_NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$PASS2_NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$PASS2_NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$PASS2_NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$PASS2_NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$PASS2_NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$PASS2_NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$PASS2_NAME")"
} >> "$PASS2_CONSOLE"
docker rm "$PASS2_NAME" >> "$PASS2_CONSOLE"
exit "$pass2_exit"
