#!/bin/bash
# One network-none probe after the install container is gone.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
ROOT=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a406/SP10/pi-runtime-kit
INSTALL_ATTEMPT=${1:-install-1}
PROBE_ATTEMPT=${2:-probe-1}
case "$INSTALL_ATTEMPT" in
  install-[0-9]|install-[0-9][0-9]) ;;
  *) echo "attempt must be install-N" >&2; exit 2 ;;
esac
case "$PROBE_ATTEMPT" in
  probe-[0-9]|probe-[0-9][0-9]) ;;
  *) echo "probe attempt must be probe-N" >&2; exit 2 ;;
esac
EVIDENCE=${ROOT}/linux/${PROBE_ATTEMPT}
KIT=${SRC}/deploy/pi-runtime-kit
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a406-pi-kit-${PROBE_ATTEMPT}
VOLUME=oo-a406-eef831aa-pi-kit

mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite probe-1 console.log" >&2
  exit 2
fi
if [ ! -s "$ROOT/linux/${INSTALL_ATTEMPT}/facts.env" ] || [ ! -s "$ROOT/linux/${INSTALL_ATTEMPT}/cjk.json" ]; then
  echo "install facts are absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! grep -qx 0 "$ROOT/linux/${INSTALL_ATTEMPT}/inside.exit"; then
  echo "install did not exit 0" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp "$KIT/linux/probe-inside.sh" "$EVIDENCE/inside.sh"
chmod 755 "$EVIDENCE/inside.sh"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
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
  echo "named probe container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if docker ps -q --filter name=oo-a406-pi-kit-install | grep -q .; then
  echo "install container is still running" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
mountpoint=$(docker volume inspect -f '{{.Mountpoint}}' "$VOLUME")
printf 'name=%s mountpoint=%s\n' "$VOLUME" "$mountpoint" > "$EVIDENCE/volume.txt"
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
  -e HOME=/tmp \
  -e KIT_VOLUME="$VOLUME" \
  -e KIT_MOUNTPOINT="$mountpoint" \
  -e INSTALL_ATTEMPT="$INSTALL_ATTEMPT" \
  -e PROBE_OUT="/evidence/linux/${PROBE_ATTEMPT}/out" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${ROOT},target=/evidence" \
  --mount "type=volume,source=${VOLUME},target=/opt/pi-runtime-kit,readonly" \
  --mount "type=bind,source=${KIT}/pack,target=/knowledge,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=64m \
  --label oo.project=open-ontology \
  --label oo.owner=a406 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a406 \
  --label oo.task=pi-runtime-kit-probe \
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
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
if [ -s "$ROOT/KIT-HANDOFF.json" ]; then
  cp "$ROOT/KIT-HANDOFF.json" "$KIT/KIT-HANDOFF.json"
  sha256sum "$ROOT/KIT-HANDOFF.json" > "$EVIDENCE/handoff.sha256"
fi
exit "$run_exit"
