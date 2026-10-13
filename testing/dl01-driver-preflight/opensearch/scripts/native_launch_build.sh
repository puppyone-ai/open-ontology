#!/bin/bash
# Start the native reader build with no CPU quota and no job cap.
# Does not start oo-dl01w1-44-rust, does not remove the C5 container, and does not delete volumes.
set -euo pipefail
ROOT="/Users/redux/puppy-one/open-ontology"
SRC="$ROOT/testing/dl01-driver-preflight/opensearch"
EVIDENCE="$ROOT/evidence/bc6cf794-2d14-4e98-bcb6-f7a9b3533a88/DL01/opensearch-driver-preflight/native-20261012"
BASE="/tmp/oo-dl01w1-44-base/Cargo.lock"
NAME="oo-dl01w1-44-native-rust"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
mkdir -p "$EVIDENCE/build"
if [ ! -f "$BASE" ]; then
  echo "base lock missing" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
  owner="$(docker inspect -f '{{index .Config.Labels "owner"}}' "$NAME")"
  status="$(docker inspect -f '{{.State.Status}}' "$NAME")"
  if [ "$owner" != "oo-dl01w1-44" ]; then
    echo "refusing to remove foreign container" >&2
    exit 1
  fi
  if [ "$status" = "running" ]; then
    echo "native rust container is already running" >&2
    exit 1
  fi
  docker rm "$NAME" >/dev/null
fi
docker run -d --name "$NAME" --platform linux/arm64 --pull never --init --user 0:0 \
  --security-opt no-new-privileges --cap-drop ALL --pids-limit 4096 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -v "$SRC:/src" \
  -v "$EVIDENCE:/evidence" \
  -v "$BASE:/base/Cargo.lock:ro" \
  -v "$SRC/scripts/native_build_inside.sh:/inside.sh:ro" \
  -v oo-dl01w1-44-cargo-reg:/usr/local/cargo/registry \
  -v oo-dl01w1-44-cargo-git:/usr/local/cargo/git \
  -v oo-dl01w1-44-target:/target \
  --label project=open-ontology \
  --label owner=oo-dl01w1-44 \
  --label session=337bb7c5-aaf7-4001-b4fe-e70de40ac91b \
  --label kind=rust-builder \
  --label task=DL01-W1-44 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$IMAGE" \
  bash /inside.sh
docker inspect -f 'NanoCpus={{.HostConfig.NanoCpus}} Memory={{.HostConfig.Memory}} CpuQuota={{.HostConfig.CpuQuota}} CpuPeriod={{.HostConfig.CpuPeriod}}' "$NAME" \
  > "$EVIDENCE/build/host-cpu.txt"
docker exec "$NAME" sh -c 'echo nproc=$(nproc); echo cpu.max=$(cat /sys/fs/cgroup/cpu.max 2>/dev/null || echo absent); echo cfs=$(cat /sys/fs/cgroup/cpu/cpu.cfs_quota_us 2>/dev/null || echo absent)' \
  > "$EVIDENCE/build/host-cgroup.txt"
set +e
status="$(docker wait "$NAME")"
wait_code=$?
set -e
printf '%s\n' "$status" > "$EVIDENCE/build/docker-wait.txt"
if [ "$wait_code" != "0" ]; then
  exit "$wait_code"
fi
exit "$status"
