#!/bin/bash
# Build only the hash-tree binary. Does not run tests or clippy.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a361
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/pg-backup-restore-review
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a361-hashbuild
WORK=/tmp/oo-a361-hash-build
BIN=/tmp/oo-a361-hash-bin/pg-backup-restore
mkdir -p "$EVIDENCE/linux/hash-build" "$WORK/cargo" "$WORK/target" /tmp/oo-a361-hash-bin
if ! docker image inspect "$RUST_IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a361 --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/linux/hash-build/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/hash-build/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named hash build container already exists" >&2
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
  --pids-limit 4096 \
  --network none \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/review \
  -e CARGO_BUILD_JOBS=1 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${REPO}/tools/pg-backup-restore,target=/src,readonly" \
  --mount "type=bind,source=${WORK}/cargo,target=/cargo-home" \
  --mount "type=bind,source=${WORK}/target,target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a361 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=pg-backup-restore-review-hash-bin \
  --label oo.platform=linux/arm64 \
  "$RUST_IMAGE" \
  cargo build --release --locked --offline --manifest-path /src/Cargo.toml --bin pg-backup-restore \
  > /tmp/oo-a361-hash-log-build.txt 2>&1
code=$?
set -e
printf '%s\n' "$code" > "$EVIDENCE/linux/hash-build/docker-run.exit"
cp /tmp/oo-a361-hash-log-build.txt "$EVIDENCE/linux/hash-build/console.log"
docker inspect "$NAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$EVIDENCE/linux/hash-build/hostconfig.txt" || true
docker rm -f "$NAME" >/dev/null
printf '%s\n' removed > "$EVIDENCE/linux/hash-build/container-removed.txt"
if [ "$code" -ne 0 ]; then
  exit "$code"
fi
cp "$WORK/target/review/release/pg-backup-restore" "$BIN"
chmod 755 "$BIN"
rm -rf "$WORK" /tmp/oo-a361-hash-log-build.txt
printf '%s\n' built > "$EVIDENCE/linux/hash-build/binary.txt"
