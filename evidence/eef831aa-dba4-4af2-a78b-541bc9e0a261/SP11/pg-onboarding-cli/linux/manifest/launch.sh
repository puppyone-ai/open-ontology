#!/bin/bash
# Inventory builder. 6g, 1 CPU, network none. Does not pull or prune.
# Origin: a199 manifest launch, cap 10, memory 6g, owner a261.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a261/SP11/pg-onboarding-cli
EVIDENCE=${REVIEW}/linux/manifest
OUT=/tmp/oo-a261-eef831aa-dba4-4af2-a78b-541bc9e0a261/manifest-out
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a261-manifest
mkdir -p "$EVIDENCE" "$OUT"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite manifest console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a261 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
rm -rf "$OUT"
mkdir -p "$OUT"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 rustc sha256_manifest" > "$EVIDENCE/invocation.txt"
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
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/sha256_manifest.rs,target=/tool/sha256_manifest.rs,readonly" \
  --mount "type=bind,source=${SRC}/tools/pg-onboarding,target=/onboarding,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence,readonly" \
  --mount "type=bind,source=${OUT},target=/out" \
  --label oo.project=open-ontology \
  --label oo.owner=a261 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a261 \
  --label oo.task=pg-onboarding-cli-manifest \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$OUT/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
cp "$OUT/console.log" "$EVIDENCE/console.log"
if [ "$run_exit" -eq 0 ]; then
  cp "$OUT/SHA256SUMS" "$REVIEW/SHA256SUMS"
  cp "$OUT/tool.txt" "$EVIDENCE/tool.txt"
  cp "$OUT/evidence.txt" "$EVIDENCE/evidence.txt"
fi
exit "$run_exit"
