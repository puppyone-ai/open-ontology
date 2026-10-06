#!/bin/bash
# One rust-builder. Retest the spike after the compile failure. Network none.
set -u
SRC=/Users/redux/puppy-one/open-ontology
PROBE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a279/SP10/pi-rpc-probe/linux/pi-live-src
PROOF=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a279/SP10/pi-rpc-probe/linux/adapter-proof-2
SPIKE=${SRC}/testing/spikes/p09-pi-rpc
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a279-pi-retest
PREFIX_VOL=oo-a279-eef831aa-pi-prefix
BIN_VOL=oo-a279-eef831aa-probe-bin
CARGO_VOL=oo-a279-eef831aa-cargo
TARGET_VOL=oo-a279-eef831aa-target

mkdir -p "$PROOF"
chmod 755 "$PROBE/inside-retest.sh" "$PROBE/launch-retest.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite adapter-proof-2 console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a279 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pi=$(docker ps -q --filter label=oo.owner=a279 --filter label=oo.kind=pi-runtime | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s own_pi=%s global=%s host_cargo=%s cap_global=10 note=retest-after-compile-failure\n' \
  "$own" "$own_pi" "$global_builders" "$host_cargo" > "$PROOF/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$PROOF/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$own_pi" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: builder capacity" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$PROOF/image.txt"
grep -q 'arch=arm64' "$PROOF/image.txt"
docker volume inspect "$PREFIX_VOL" >/dev/null
docker volume inspect "$BIN_VOL" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$PROOF/invocation.txt"
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
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_NET_OFFLINE=true \
  --mount "type=bind,source=${PROBE}/inside-retest.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SPIKE},target=/work" \
  --mount "type=bind,source=${PROOF},target=/proof" \
  --mount "type=volume,source=${PREFIX_VOL},target=/opt/pi-prefix,readonly" \
  --mount "type=volume,source=${BIN_VOL},target=/opt/probe,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --tmpfs /var/pi-home:rw,nosuid,nodev,size=512m \
  --label oo.project=open-ontology \
  --label oo.owner=a279 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a279 \
  --label oo.task=pi-rpc-probe-retest \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROOF/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROOF/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$PROOF/hostconfig.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$PROOF/mounts.txt"
docker rm "$NAME" > "$PROOF/container-removed.txt"
exit "$run_exit"
