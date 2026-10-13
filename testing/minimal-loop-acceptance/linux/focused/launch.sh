#!/bin/bash
# Focused Linux arm64 Rust 1.94 tests. One 6 GiB builder. No pull. No prune.
# Origin launch shape: testing/product-benchmark/linux/smoke/launch.sh
# Origin commit 6cec19fa15932fd5e2ee94070857ead464887f97
# Origin blob b79240d4e9b5be5146f75339e518667a4477e623
set -u
SRC=/Users/redux/puppy-one/open-ontology
ATTEMPT=${1:-focused}
case "$ATTEMPT" in
  focused|focused-2|focused-3|focused-4|focused-5|focused-6|focused-7|focused-8) ;;
  *) echo "attempt must be focused through focused-8" >&2; exit 2 ;;
esac
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a407/SP11/minimal-loop-acceptance/linux/${ATTEMPT}
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a407-${ATTEMPT}
NET=oo-a407-eef831aa-net
CARGO_VOL=oo-a407-eef831aa-cargo
TARGET_VOL=oo-a407-eef831aa-target
KIT_VOL=oo-a406-eef831aa-pi-kit
mkdir -p "$EVIDENCE/phases"
chmod 755 "$SRC/testing/minimal-loop-acceptance/linux/focused/inside.sh" "$SRC/testing/minimal-loop-acceptance/linux/focused/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite focused console.log" >&2
  exit 2
fi
if [ "$ATTEMPT" = "focused-5" ] && ! docker volume inspect "$KIT_VOL" >/dev/null 2>&1; then
  echo "a406 kit volume is absent; not creating it" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a407 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a407 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a407 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a407 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a407 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a407 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a407 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named focused container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
cmd=(
  docker run --name "$NAME"
  --platform linux/arm64
  --pull never
  --init
  --user 0:0
  --security-opt no-new-privileges
  --cap-drop ALL
  --memory 6g
  --memory-swap 6g
  --cpus 1
  --pids-limit 256
  --network "$NET"
  -e HOME=/tmp
  -e RUSTUP_TOOLCHAIN=1.94.0
  --mount "type=bind,source=${SRC},target=/src,readonly"
  --mount "type=bind,source=${EVIDENCE},target=/evidence"
  --mount "type=bind,source=${SRC}/testing/minimal-loop-acceptance/linux/focused/inside.sh,target=/inside.sh,readonly"
  --mount "type=volume,source=${CARGO_VOL},target=/cargo"
  --mount "type=volume,source=${TARGET_VOL},target=/target"
)
if [ "$ATTEMPT" = "focused-5" ]; then
  cmd+=(--mount "type=volume,source=${KIT_VOL},target=/opt/pi-runtime-kit,readonly" -e KIT_REQUIRED=1)
fi
cmd+=(
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m
  --label oo.project=open-ontology
  --label oo.owner=a407
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a407
  --label oo.task=minimal-loop-acceptance-focused
  --label oo.toolchain=1.94.0
  --label oo.platform=linux/arm64
  --label oo.kind=rust-builder
  "$IMAGE"
  bash /inside.sh
)
set +e
"${cmd[@]}" > "$EVIDENCE/console.log" 2>&1
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
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
