#!/bin/bash
# Two sequential hash containers. The second manifest includes the metadata
# written after the first. This script does not write evidence files after
# the second sha256 run.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a195/SP06/parser-worker-review
HASH=${EVIDENCE}/linux/hash
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a195-hash
NET=oo-a195-eef831aa-net
CARGO_VOL=oo-a195-eef831aa-cargo

chmod 755 "$HASH/inside.sh" "$HASH/launch.sh"
mkdir -p "$HASH"
if [ -f "$HASH/console.log" ]; then
  echo "refusing to overwrite hash console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a195 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi

run_hash() {
  docker run --name "$NAME" \
    --platform linux/arm64 \
    --pull never \
    --init \
    --user 0:0 \
    --security-opt no-new-privileges:true \
    --cap-drop ALL \
    --memory 8g \
    --memory-swap 8g \
    --cpus 4 \
    --pids-limit 256 \
    --network "$NET" \
    -e HOME=/tmp \
    -e CARGO_HOME=/cargo-home \
    --mount "type=bind,source=${HASH}/inside.sh,target=/inside.sh,readonly" \
    --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
    --mount "type=bind,source=${EVIDENCE},target=/evidence" \
    --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
    --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
    --label oo.project=open-ontology \
    --label oo.owner=a195 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a195 \
    --label oo.task=parser-worker-review-hash \
    --label oo.toolchain=1.94.0 \
    --label oo.platform=linux/arm64 \
    --label oo.kind=rust-builder \
    "$IMAGE" \
    bash /inside.sh
}

printf '%s\n' "phase=hash image=${IMAGE} container=${NAME}" > "$HASH/invocation.txt"
set +e
run_hash > "$HASH/console.log" 2>&1
first=$?
set +e
printf '%s\n' "$first" > "$HASH/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$HASH/hostconfig.txt"
docker rm "$NAME" > "$HASH/container-removed.txt"
printf '%s\n' "$first" > "$HASH/tool.exit"
if [ "$first" -ne 0 ]; then
  exit "$first"
fi
set +e
run_hash > /tmp/oo-a195-eef831aa-dba4-4af2-a78b-541bc9e0a195/hash-refresh.log 2>&1
second=$?
set +e
docker rm "$NAME" >/dev/null
exit "$second"
