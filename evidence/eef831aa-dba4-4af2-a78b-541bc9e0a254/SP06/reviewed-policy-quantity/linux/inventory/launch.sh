#!/bin/bash
# Compile tools/sha256.rs and write SHA256SUMS for this evidence directory.
# Does not overwrite an existing console.log. Does not prune.
# Origin: a250 inventory launch, git hash-object c82a332f25079d6c9b453a881b011cadeffdbcb5
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a250/SP12/trial-report-adapter/linux/inventory/launch.sh
# Delta: owner a254, sha256 dir, edition 2021, --network none, pids 4096, no crate mount.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a254/SP06/reviewed-policy-quantity
INV=${EVIDENCE}/linux/inventory
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a254-hash
CONSOLE=/tmp/oo-a254-eef831aa-dba4-4af2-a78b-541bc9e0a254/hash-console.log

chmod 755 "$INV/launch.sh"
if [ -f "$INV/console.log" ]; then
  echo "refusing to overwrite inventory console.log" >&2
  exit 2
fi
mkdir -p "$(dirname "$CONSOLE")"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$INV/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a254 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$INV/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$INV/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named hash container already exists" >&2
  printf '%s\n' 2 > "$INV/docker-run.exit"
  exit 2
fi
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
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=bind,source=${EVIDENCE}/tools/sha256.rs,target=/tool/sha256.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --label oo.project=open-ontology \
  --label oo.owner=a254 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a254 \
  --label oo.task=reviewed-policy-quantity-inventory \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'rustc --edition 2021 -C debuginfo=0 -o /sha256 /tool/sha256.rs && /sha256 file /tool/sha256.rs && /sha256 dir /evidence/SHA256SUMS /evidence' \
  > "$CONSOLE" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$INV/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$INV/hostconfig.txt"
docker rm "$NAME" > "$INV/container-removed.txt"
cp "$CONSOLE" "$INV/console.log"
exit "$run_exit"
