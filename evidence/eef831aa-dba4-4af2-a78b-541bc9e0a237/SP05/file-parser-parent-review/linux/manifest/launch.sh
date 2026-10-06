#!/bin/bash
# Compile and run the std-only manifest tool.
# Stdout paths must sit outside this review directory. The final container
# rewrites MANIFEST.sha256 and writes nothing else here. Does not prune. Does not pull.
set -u
if [ "$#" -ne 3 ]; then
  echo "usage: launch.sh <first-console-outside> <rewrite-console-outside> <final-console-outside>" >&2
  exit 2
fi
CONSOLE=$1
SECOND=$2
FINAL=$3
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a237/SP05/file-parser-parent-review
EVIDENCE=${REVIEW}/linux/manifest
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a237-manifest
REWRITE=oo-a237-manifest-rewrite
LAST=oo-a237-manifest-final
case "$CONSOLE" in
  "$REVIEW"/*) echo "first console path is inside the review directory" >&2; exit 2 ;;
esac
case "$SECOND" in
  "$REVIEW"/*) echo "rewrite console path is inside the review directory" >&2; exit 2 ;;
esac
case "$FINAL" in
  "$REVIEW"/*) echo "final console path is inside the review directory" >&2; exit 2 ;;
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

gate() {
  local own global_builders host_cargo
  own=$(docker ps -q --filter label=oo.owner=a237 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
  printf 'own=%s global=%s host_cargo=%s cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo"
  if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
    echo "deferred: rust builder capacity" >&2
    return 3
  fi
  return 0
}

gate > "$EVIDENCE/capacity.txt" || {
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
}
if docker container inspect "$NAME" >/dev/null 2>&1 \
  || docker container inspect "$REWRITE" >/dev/null 2>&1 \
  || docker container inspect "$LAST" >/dev/null 2>&1
then
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

run_hash() {
  local name=$1
  local dest=$2
  docker run --name "$name" \
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
    --label oo.owner=a237 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a237 \
    --label oo.task=file-parser-parent-review-manifest \
    --label oo.toolchain=1.94.0 \
    --label oo.platform=linux/arm64 \
    --label oo.kind=rust-builder \
    "$IMAGE" \
    bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256' \
    > "$dest" 2>&1
}

set +e
run_hash "$NAME" "$CONSOLE"
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

gate >> "$CONSOLE" || exit 3
set +e
run_hash "$REWRITE" "$SECOND"
rewrite_exit=$?
set +e
{
  echo "rewrite_exit=$rewrite_exit"
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$REWRITE")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$REWRITE")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$REWRITE")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$REWRITE")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$REWRITE")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$REWRITE")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$REWRITE")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$REWRITE")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$REWRITE")"
} > "$EVIDENCE/rewrite-hostconfig.txt"
cp "$SECOND" "$EVIDENCE/rewrite-console.log"
docker rm "$REWRITE" > "$EVIDENCE/rewrite-removed.txt"
if [ "$rewrite_exit" -ne 0 ]; then
  exit "$rewrite_exit"
fi

gate >> "$SECOND" || exit 3
set +e
run_hash "$LAST" "$FINAL"
final_exit=$?
set +e
{
  echo "final_exit=$final_exit"
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$LAST")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$LAST")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$LAST")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$LAST")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$LAST")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$LAST")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$LAST")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$LAST")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$LAST")"
} >> "$FINAL"
docker rm "$LAST" >> "$FINAL"
exit "$final_exit"
