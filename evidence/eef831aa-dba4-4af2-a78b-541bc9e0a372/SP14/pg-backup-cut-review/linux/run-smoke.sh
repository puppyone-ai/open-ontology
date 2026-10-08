#!/bin/bash
# One short Rust 1.94 builder for the pre-edit smoke. Does not pull.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a372
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/pg-backup-cut-review
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
BNAME=oo-a372-smoke
FIXED=20f065096fa9e20414eed8951d830333160e4af6

mkdir -p "$EVIDENCE/linux/pre-edit-smoke" "$EVIDENCE/origin"
chmod 755 "$EVIDENCE/linux/smoke-inside.sh" "$EVIDENCE/linux/run-smoke.sh"
if [ -f "$EVIDENCE/linux/pre-edit-smoke/inside.exit" ]; then
  echo "pre-edit smoke evidence already exists" >&2
  exit 2
fi
git -C "$REPO" rev-parse "${FIXED}:tools/pg-backup-restore/src/gold.rs" > "$EVIDENCE/origin/gold-blob.txt"
git -C "$REPO" rev-parse "1d88fe8ad9fa4a113c105e1df4d0903e517fc9a9:tools/pg-backup-restore/src/gold.rs" > "$EVIDENCE/origin/gold-blob-a361.txt"
git -C "$REPO" diff --stat "$FIXED" -- tools/pg-backup-restore > "$EVIDENCE/origin/product-tree.diff"
if [ -s "$EVIDENCE/origin/product-tree.diff" ]; then
  echo "product tree differs from the fixed commit" >&2
  exit 2
fi
cmp -s "$EVIDENCE/origin/gold-blob.txt" "$EVIDENCE/origin/gold-blob-a361.txt"
if ! docker image inspect "$RUST_IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/linux/pre-edit-smoke/docker-run.exit"
  exit 1
fi
docker image inspect "$RUST_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/linux/pre-edit-smoke/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/linux/pre-edit-smoke/image.txt"
own=$(docker ps -q --filter label=oo.owner=a372 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/linux/pre-edit-smoke/capacity.txt"
printf '%s\n' "$global_builders" > "$EVIDENCE/linux/pre-edit-smoke/builders-before.txt"
printf '%s\n' "$host_cargo" > "$EVIDENCE/linux/pre-edit-smoke/host-cargo.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/pre-edit-smoke/docker-run.exit"
  exit 3
fi
if docker container inspect "$BNAME" >/dev/null 2>&1; then
  echo "named smoke builder already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/pre-edit-smoke/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${BNAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 pinned-rust-1.94 bash /smoke-inside.sh" > "$EVIDENCE/linux/pre-edit-smoke/invocation.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/linux/pre-edit-smoke/fixed-commit.txt"
set +e
docker run --name "$BNAME" \
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
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${REPO}/tools/pg-backup-restore,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/linux/smoke-inside.sh,target=/smoke-inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --label oo.project=open-ontology \
  --label oo.owner=a372 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=pg-backup-cut-review-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  bash /smoke-inside.sh > "$EVIDENCE/linux/pre-edit-smoke/console.log" 2>&1
code=$?
set -e
printf '%s\n' "$code" > "$EVIDENCE/linux/pre-edit-smoke/docker-run.exit"
docker inspect "$BNAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$EVIDENCE/linux/pre-edit-smoke/hostconfig.txt" || true
docker inspect "$BNAME" --format '{{json .Mounts}}' > "$EVIDENCE/linux/pre-edit-smoke/mounts.txt" || true
docker rm -f "$BNAME" >/dev/null
printf '%s\n' removed > "$EVIDENCE/linux/pre-edit-smoke/container-removed.txt"
if [ "$code" -ne 0 ]; then
  echo "smoke exit ${code}" >&2
  exit "$code"
fi
