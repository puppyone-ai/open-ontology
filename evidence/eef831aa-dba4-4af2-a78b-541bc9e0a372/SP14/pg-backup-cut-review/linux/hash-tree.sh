#!/bin/bash
# Hash this evidence tree with the fixed product binary. Run only after cleanup-done.
# One rust-builder, network none, no cargo cache, no host home. Does not pull or prune.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a372
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/pg-backup-cut-review
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
BIN=/tmp/oo-a372-hash-bin/pg-backup-restore
BNAME=oo-a372-hash
CONSOLE=/tmp/oo-a372-hash-console.txt

mkdir -p "$EVIDENCE/linux/hash"
if [ ! -f "$EVIDENCE/cleanup-done.txt" ] || [ "$(cat "$EVIDENCE/cleanup-done.txt")" != done ]; then
  echo "cleanup-done is missing" >&2
  exit 2
fi
if [ ! -x "$BIN" ]; then
  echo "hash binary is missing" >&2
  exit 2
fi
if ! docker image inspect "$RUST_IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/linux/hash/docker-run.exit"
  exit 1
fi
docker image inspect "$RUST_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/linux/hash/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/linux/hash/image.txt"
own=$(docker ps -q --filter label=oo.owner=a372 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/linux/hash/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/hash/docker-run.exit"
  exit 3
fi
if docker container inspect "$BNAME" >/dev/null 2>&1; then
  owner=$(docker inspect --format '{{index .Config.Labels "oo.owner"}}' "$BNAME")
  session=$(docker inspect --format '{{index .Config.Labels "oo.session"}}' "$BNAME")
  if [ "$owner" != a372 ] || [ "$session" != "$RUN_ID" ]; then
    echo "hash container name is foreign" >&2
    exit 2
  fi
  docker rm -f "$BNAME" >/dev/null
fi
printf '%s\n' "docker run --name ${BNAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 pinned-rust-1.94 pg-backup-restore hash-tree" > "$EVIDENCE/linux/hash/invocation.txt"
set +e
docker run --name "$BNAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 256 \
  --network none \
  --user "$(id -u):$(id -g)" \
  -e HOME=/tmp \
  -e PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${BIN},target=/usr/local/bin/pg-backup-restore,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a372 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=pg-backup-cut-review \
  "$RUST_IMAGE" \
  /usr/local/bin/pg-backup-restore hash-tree /evidence /evidence/SHA256SUMS \
  > "$CONSOLE" 2>&1
code=$?
set -e
printf '%s\n' "$code" > "$EVIDENCE/linux/hash/docker-run.exit"
owner=$(docker inspect --format '{{index .Config.Labels "oo.owner"}}' "$BNAME")
session=$(docker inspect --format '{{index .Config.Labels "oo.session"}}' "$BNAME")
if [ "$owner" = a372 ] && [ "$session" = "$RUN_ID" ]; then
  docker rm -f "$BNAME" >/dev/null
  printf '%s\n' removed > "$EVIDENCE/linux/hash/container-removed.txt"
fi
if [ ! -f "$EVIDENCE/linux/hash/console.log" ]; then
  cp "$CONSOLE" "$EVIDENCE/linux/hash/console.log"
fi
if [ "$code" -ne 0 ]; then
  echo "hash-tree exit ${code}" >&2
  exit "$code"
fi
echo "hash-tree exit 0"
