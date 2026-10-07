#!/bin/bash
# Hash this evidence tree with the linux/arm64 review binary. One builder. No pull.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a361
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/pg-backup-restore-review
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
BIN=/tmp/oo-a361-hash-bin/pg-backup-restore
PASS=${1:?pass name}
NAME=oo-a361-hash
if [ ! -x "$BIN" ]; then
  echo "hash binary is missing" >&2
  exit 1
fi
if [ ! -f "$EVIDENCE/cleanup-done.txt" ] || [ "$(cat "$EVIDENCE/cleanup-done.txt")" != done ]; then
  echo "cleanup-done.txt is missing" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/linux/${PASS}" /tmp/oo-a361-hash-log
own=$(docker ps -q --filter label=oo.owner=a361 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/linux/${PASS}/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/${PASS}/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named hash container already exists" >&2
  exit 2
fi
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 512 \
  --network none \
  -e HOME=/tmp \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${BIN},target=/opt/pg-backup-restore,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a361 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=pg-backup-restore-review-hash \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  /opt/pg-backup-restore hash-tree /evidence /evidence/SHA256SUMS \
  > /tmp/oo-a361-hash-log/console.log 2>&1
code=$?
set -e
printf '%s\n' "$code" > /tmp/oo-a361-hash-log/docker-run.exit
docker inspect "$NAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > /tmp/oo-a361-hash-log/hostconfig.txt || true
docker rm -f "$NAME" >/dev/null
printf '%s\n' removed > /tmp/oo-a361-hash-log/container-removed.txt
cp /tmp/oo-a361-hash-log/console.log /tmp/oo-a361-hash-log/docker-run.exit /tmp/oo-a361-hash-log/hostconfig.txt /tmp/oo-a361-hash-log/container-removed.txt "$EVIDENCE/linux/${PASS}/"
exit "$code"
