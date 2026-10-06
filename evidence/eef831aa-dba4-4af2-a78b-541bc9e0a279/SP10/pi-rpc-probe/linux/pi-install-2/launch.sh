#!/bin/bash
# One private Pi install container. Pinned Rust image supplies the OS only.
# Node is the checksum-verified public binary. No host npm cache. No prune.
# Origin: a279 preedit launch, which records a204/a200/a199/a197 recipe hashes.
# Changes from that launch: kind=pi-runtime, npm and prefix volumes, no cargo,
# network is required for the public downloads, memory 6g, cpus 1.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a279/SP10/pi-rpc-probe/linux/pi-install-2
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a279-pi-install-2
NET=oo-a279-eef831aa-net
NPM_VOL=oo-a279-eef831aa-npm-cache
PREFIX_VOL=oo-a279-eef831aa-pi-prefix

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite pi-install console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a279 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a279 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
own_rust=$(docker ps -q --filter label=oo.owner=a279 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pi=$(docker ps -q --filter label=oo.owner=a279 --filter label=oo.kind=pi-runtime | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own_rust=%s own_pi=%s global=%s host_cargo=%s cap_global=10 cap_pi=1\n' \
  "$own_rust" "$own_pi" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own_rust" -ge 1 ] || [ "$own_pi" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: builder or pi runtime capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker volume inspect "$NPM_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a279 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a279 \
  --label oo.kind=npm-cache \
  "$NPM_VOL" >/dev/null
docker volume inspect "$PREFIX_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a279 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a279 \
  --label oo.kind=pi-prefix \
  "$PREFIX_VOL" >/dev/null
docker volume inspect --format '{{.Name}}' "$NPM_VOL" > "$EVIDENCE/volume-npm.txt"
docker volume inspect --format '{{.Name}}' "$PREFIX_VOL" > "$EVIDENCE/volume-prefix.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named pi-install container already exists" >&2
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
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/var/pi-home \
  -e GIT_TERMINAL_PROMPT=0 \
  -e NPM_CONFIG_CACHE=/npm-cache \
  -e NPM_CONFIG_UPDATE_NOTIFIER=false \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${NPM_VOL},target=/npm-cache" \
  --mount "type=volume,source=${PREFIX_VOL},target=/opt/pi-prefix" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a279 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a279 \
  --label oo.task=pi-rpc-probe-install-2 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=pi-runtime \
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
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
