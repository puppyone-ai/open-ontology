#!/bin/bash
# One builder. Replaces only bin/kit and its recorded hash.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
ATTEMPT=${1:-rebuild-1}
case "$ATTEMPT" in
  rebuild-[0-9]|rebuild-[0-9][0-9]) ;;
  *) echo "attempt must be rebuild-N" >&2; exit 2 ;;
esac
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a406/SP10/pi-runtime-kit/linux/${ATTEMPT}
KIT=${SRC}/deploy/pi-runtime-kit
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a406-pi-kit-${ATTEMPT}
NET=oo-a406-pi-kit-net
VOLUME=oo-a406-eef831aa-pi-kit
CARGO_VOLUME=oo-a406-eef831aa-pi-cargo
TARGET_VOLUME=oo-a406-eef831aa-pi-target

mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite rebuild-1 console.log" >&2
  exit 2
fi
cp "$KIT/linux/rebuild-inside.sh" "$EVIDENCE/inside.sh"
cp "$KIT/linux/rebuild-launch.sh" "$EVIDENCE/launch.sh"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
own=$(docker ps -q --filter label=oo.owner=a406 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_task=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named rebuild container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect -f 'driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
grep -q 'driver=bridge' "$EVIDENCE/network.txt"
grep -q 'internal=false' "$EVIDENCE/network.txt"
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
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SRC},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a406/SP10/pi-runtime-kit/linux/install-6,target=/install6" \
  --mount "type=volume,source=${VOLUME},target=/opt/pi-runtime-kit" \
  --mount "type=volume,source=${CARGO_VOLUME},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOLUME},target=/target" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
  --label oo.project=open-ontology \
  --label oo.owner=a406 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a406 \
  --label oo.task=pi-runtime-kit-rebuild \
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
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
