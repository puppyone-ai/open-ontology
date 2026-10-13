#!/bin/bash
# One private arm64 builder. Downloads stay in this container.
# The kit volume is the export. It is not copied into git.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
ATTEMPT=${1:-install-1}
case "$ATTEMPT" in
  install-[0-9]|install-[0-9][0-9]) ;;
  *) echo "attempt must be install-N" >&2; exit 2 ;;
esac
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a406/SP10/pi-runtime-kit/linux/${ATTEMPT}
KIT=${SRC}/deploy/pi-runtime-kit
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a406-pi-kit-${ATTEMPT}
NET=oo-a406-pi-kit-net
VOLUME=oo-a406-eef831aa-pi-kit
CARGO_VOLUME=oo-a406-eef831aa-pi-cargo
TARGET_VOLUME=oo-a406-eef831aa-pi-target
NPM_VOLUME=oo-a406-eef831aa-pi-npm

mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite install-1 console.log" >&2
  exit 2
fi
cp "$KIT/linux/install-inside.sh" "$EVIDENCE/inside.sh"
cp "$KIT/linux/libclang.sh" "$EVIDENCE/libclang.sh"
cp "$KIT/linux/install-launch.sh" "$EVIDENCE/launch.sh"
cp "$KIT/pins.env" "$EVIDENCE/pins.env"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/libclang.sh" "$EVIDENCE/launch.sh"
sha256sum "$EVIDENCE/inside.sh" "$EVIDENCE/libclang.sh" "$EVIDENCE/pins.env" > "$EVIDENCE/script-sha256.txt"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a406 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_task=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named install container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --driver bridge \
    --label oo.project=open-ontology \
    --label oo.owner=a406 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a406 \
    "$NET" > "$EVIDENCE/network-create.txt"
fi
docker network inspect -f 'driver={{.Driver}} internal={{.Internal}} name={{.Name}}' "$NET" > "$EVIDENCE/network.txt"
grep -q 'driver=bridge' "$EVIDENCE/network.txt"
grep -q 'internal=false' "$EVIDENCE/network.txt"

for volume in "$VOLUME" "$CARGO_VOLUME" "$TARGET_VOLUME" "$NPM_VOLUME"; do
  if ! docker volume inspect "$volume" >/dev/null 2>&1; then
    docker volume create \
      --label oo.project=open-ontology \
      --label oo.owner=a406 \
      --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a406 \
      "$volume" >/dev/null
  fi
done

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
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e GIT_TERMINAL_PROMPT=0 \
  -e KIT_VOLUME="$VOLUME" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/libclang.sh,target=/libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/pins.env,target=/pins.env,readonly" \
  --mount "type=bind,source=${SRC},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${VOLUME},target=/opt/pi-runtime-kit" \
  --mount "type=volume,source=${CARGO_VOLUME},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOLUME},target=/target" \
  --mount "type=volume,source=${NPM_VOLUME},target=/npm-cache" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a406 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a406 \
  --label oo.task=pi-runtime-kit-install \
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
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker volume inspect -f 'name={{.Name}} mountpoint={{.Mountpoint}}' "$VOLUME" > "$EVIDENCE/volume.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-after.txt"
exit "$run_exit"
