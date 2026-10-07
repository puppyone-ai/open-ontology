#!/bin/bash
# Inventory launcher. Writes inspect files before the hash, then adds nothing.
set -u
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a319/SP00/gateway-dependencies-review
EVIDENCE=${BASE}/linux/inventory
ROOT=/tmp/oo-a319-eef831aa-dba4-4af2-a78b-541bc9e0a319/inventory
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a319-inventory
NET=oo-a319-eef831aa-net

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$BASE/SHA256SUMS" ]; then
  echo "refusing to overwrite SHA256SUMS" >&2
  exit 2
fi
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite inventory console" >&2
  exit 2
fi
mkdir -p "$ROOT"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a319 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named inventory container already exists" >&2
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
printf '%s\n' "Manifest root is the review directory. sha256_manifest skips SHA256SUMS only. Proof hostconfig, reports, and this inventory's inspect files are written before the walk. On success the host does not add files after the walk." > "$EVIDENCE/scope.txt"
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
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${BASE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${BASE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a319 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a319 \
  --label oo.task=gateway-dependencies-review-inventory \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$ROOT/console.log" 2>&1 &
run_pid=$!
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  if docker container inspect "$NAME" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" -ne 1 ]; then
  echo "inventory container did not start" >&2
  wait "$run_pid"
  exit 1
fi
{
  echo "Observe=running-before-hash"
  echo "Status=$(docker inspect -f '{{.State.Status}}' "$NAME")"
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
printf '%s\n' ready > "$EVIDENCE/host-meta-ready"
wait "$run_pid"
run_exit=$?
final_exit=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")
final_oom=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")
final_status=$(docker inspect -f '{{.State.Status}}' "$NAME")
printf 'final_exit=%s final_oom=%s final_status=%s launch_exit=%s\n' \
  "$final_exit" "$final_oom" "$final_status" "$run_exit" >> "$ROOT/console.log"
if [ "$run_exit" -ne 0 ] || [ "$final_exit" != "0" ] || [ "$final_oom" != "false" ]; then
  echo "inventory container failed; evidence files were not rewritten" >&2
  exit 1
fi
removed=$(docker rm "$NAME")
if [ "$removed" != "oo-a319-inventory" ]; then
  echo "docker rm returned ${removed}" >&2
  exit 1
fi
exit 0
