#!/bin/bash
# Hash this evidence tree inside the pinned linux/arm64 Rust image. One builder. No pull.
# Origin shape: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a361/SP14/pg-backup-restore-review/linux/hash-tree.sh
# sha256 c96add55ffa408b52112553345a698d36f80b6f60496a70aa45ff07d25ee2dc5
# Change: std-only oracle at linux/hash-oracle/main.rs. No product binary. a365 names.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a365
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/control-migration-coexistence
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PASS=${1:?pass name}
NAME=oo-a365-hash
if [ ! -f "$EVIDENCE/cleanup-done.txt" ] || [ "$(cat "$EVIDENCE/cleanup-done.txt")" != done ]; then
  echo "cleanup-done.txt is missing" >&2
  exit 2
fi
if [ ! -f "$EVIDENCE/linux/hash-oracle/main.rs" ]; then
  echo "hash oracle is missing" >&2
  exit 2
fi
mkdir -p "$EVIDENCE/linux/${PASS}" /tmp/oo-a365-hash-log
own=$(docker ps -q --filter label=oo.owner=a365 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
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
  --label oo.project=open-ontology \
  --label oo.owner=a365 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=control-migration-coexistence-hash \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  bash -c 'rustc --edition 2021 -C opt-level=2 -o /tmp/hash-tree /evidence/linux/hash-oracle/main.rs && /tmp/hash-tree /evidence /evidence/SHA256SUMS' \
  > /tmp/oo-a365-hash-log/console.log 2>&1
code=$?
set -e
printf '%s\n' "$code" > /tmp/oo-a365-hash-log/docker-run.exit
docker inspect "$NAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > /tmp/oo-a365-hash-log/hostconfig.txt || true
docker rm -f "$NAME" >/dev/null
printf '%s\n' removed > /tmp/oo-a365-hash-log/container-removed.txt
cp /tmp/oo-a365-hash-log/console.log /tmp/oo-a365-hash-log/docker-run.exit /tmp/oo-a365-hash-log/hostconfig.txt /tmp/oo-a365-hash-log/container-removed.txt "$EVIDENCE/linux/${PASS}/"
exit "$code"
