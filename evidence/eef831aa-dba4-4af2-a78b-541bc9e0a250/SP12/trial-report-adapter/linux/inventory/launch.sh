#!/bin/bash
# Compile the copied a199 hasher and write the crate and evidence inventories.
# Does not overwrite an existing console.log. Does not prune.
# Origin: a225 inventory launch 934c8098703b6462c4f774878d1a4241bd84905d
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a225/SP12/trial-statistics/linux/inventory/launch.sh
# sha256 f7807489580a7ffd55e1bbf9d8c042bdeb1d69264ff8bca4eda72097cc515e80
# Delta: owner a250, crate benchmarks/trial-report-adapter, private network and tmp.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a250/SP12/trial-report-adapter
INV=${EVIDENCE}/linux/inventory
CRATE=${SRC}/benchmarks/trial-report-adapter
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a250-hash
NET=oo-a250-eef831aa-net
CONSOLE=/tmp/oo-a250-eef831aa-dba4-4af2-a78b-541bc9e0a250/hash-console.log

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
docker network inspect "$NET" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a250 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=bind,source=${EVIDENCE}/tools/sha256_manifest.rs,target=/tool/sha256_manifest.rs,readonly" \
  --mount "type=bind,source=${CRATE},target=/crate,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --label oo.project=open-ontology \
  --label oo.owner=a250 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a250 \
  --label oo.task=trial-report-adapter-inventory \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'rustc --edition 2021 -C debuginfo=0 -o /sha256_manifest /tool/sha256_manifest.rs && /sha256_manifest /crate /evidence/crate-sha256.txt && /sha256_manifest /evidence /evidence/SHA256SUMS.txt' \
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
