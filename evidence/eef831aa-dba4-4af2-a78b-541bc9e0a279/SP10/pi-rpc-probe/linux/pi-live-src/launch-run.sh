#!/bin/bash
# One private Pi runtime container. Network none. Uses the compiled probe.
set -u
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: launch-run.sh abort|restore [delta]" >&2
  exit 2
fi
MODE=$1
SUFFIX=${2:-}
if [ "$MODE" != "abort" ] && [ "$MODE" != "restore" ]; then
  echo "mode must be abort or restore" >&2
  exit 2
fi
if [ -n "$SUFFIX" ] && [ "$SUFFIX" != "delta" ]; then
  echo "suffix must be empty or delta" >&2
  exit 2
fi
TAG=$MODE
if [ -n "$SUFFIX" ]; then
  TAG=${MODE}-${SUFFIX}
fi
SRC=/Users/redux/puppy-one/open-ontology
PROBE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a279/SP10/pi-rpc-probe/linux/pi-live-src
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a279/SP10/pi-rpc-probe/linux/pi-live-${TAG}
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a279-pi-live-${TAG}
PREFIX_VOL=oo-a279-eef831aa-pi-prefix
BIN_VOL=oo-a279-eef831aa-probe-bin

mkdir -p "$EVIDENCE"
printf '%s\n' "$MODE" > "$EVIDENCE/mode.txt"
chmod 755 "$PROBE/inside-run.sh" "$PROBE/launch-run.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite ${TAG} console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a279 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pi=$(docker ps -q --filter label=oo.owner=a279 --filter label=oo.kind=pi-runtime | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s own_pi=%s global=%s host_cargo=%s cap_global=10 cap_pi=1 mode=%s\n' \
  "$own" "$own_pi" "$global_builders" "$host_cargo" "$TAG" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$own_pi" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: builder or pi runtime capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker volume inspect "$PREFIX_VOL" >/dev/null
docker volume inspect "$BIN_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  --pids-limit 512 \
  --network none \
  -e HOME=/var/pi-home \
  --mount "type=bind,source=${PROBE}/inside-run.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${PREFIX_VOL},target=/opt/pi-prefix,readonly" \
  --mount "type=volume,source=${BIN_VOL},target=/opt/probe,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
  --tmpfs /var/pi-home:rw,nosuid,nodev,size=512m \
  --label oo.project=open-ontology \
  --label oo.owner=a279 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a279 \
  --label oo.task=pi-rpc-probe-live-${TAG} \
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
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
