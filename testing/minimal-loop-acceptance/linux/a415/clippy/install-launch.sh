#!/bin/bash
# Private bridge install of Rust 1.94 Clippy. Not the acceptance judge.
# No host network, no host HOME, no secrets, no pull, no prune, no Postgres.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a415/SP11/minimal-loop-acceptance/linux/clippy-install
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a415-clippy-install
RUSTUP_VOL=oo-a415-eef831aa-rustup
NET=oo-a415-eef831aa-net
INSIDE=${SRC}/testing/minimal-loop-acceptance/linux/a415/clippy/install-inside.sh
mkdir -p "$EVIDENCE/phases"
chmod 755 "$INSIDE" "$SRC/testing/minimal-loop-acceptance/linux/a415/clippy/install-launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite clippy-install console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker volume inspect "$RUSTUP_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a415 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a415 \
  --label oo.kind=volume \
  "$RUSTUP_VOL" >/dev/null
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a415 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a415 \
    --label oo.kind=bridge \
    "$NET" >/dev/null
fi
own=$(docker ps -q --filter label=oo.owner=a415 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
docker network inspect --format '{{.Driver}}' "$NET" > "$EVIDENCE/network-driver.txt"
grep -qx bridge "$EVIDENCE/network-driver.txt" || { echo "install network is not a bridge" >&2; printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"; exit 2; }
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named install container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${INSIDE},target=/inside.sh,readonly" \
  --mount "type=volume,source=${RUSTUP_VOL},target=/rustup" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a415 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a415 \
  --label oo.task=minimal-loop-acceptance-clippy-install \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
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
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
