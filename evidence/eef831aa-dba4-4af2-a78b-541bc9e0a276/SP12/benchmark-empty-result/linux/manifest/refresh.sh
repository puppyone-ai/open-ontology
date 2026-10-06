#!/bin/bash
# Rewrite MANIFEST.sha256 after linux/manifest/console.log exists.
# Stdout stays outside this evidence directory. This script does not write
# hostconfig, console.log, or exit files into the evidence tree.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a267/SP12/question-bank-suite/linux/manifest/refresh.sh
# Delta: owner a276.
set -u
if [ "$#" -ne 1 ]; then
  echo "usage: refresh.sh <console-outside-evidence-dir>" >&2
  exit 2
fi
CONSOLE=$1
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a276/SP12/benchmark-empty-result
EVIDENCE=${REVIEW}/linux/manifest
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a276-manifest-refresh
case "$CONSOLE" in
  "$REVIEW"/*)
    echo "console path is inside the evidence directory" >&2
    exit 2
    ;;
esac
if [ ! -f "$EVIDENCE/console.log" ]; then
  echo "manifest console.log is missing" >&2
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a276 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named refresh container already exists" >&2
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
  --pids-limit 2048 \
  --network none \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --label oo.project=open-ontology \
  --label oo.owner=a276 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a276 \
  --label oo.task=benchmark-empty-result-manifest-refresh \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2021 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/MANIFEST.sha256' \
  > "$CONSOLE" 2>&1
run_exit=$?
set +e
docker rm "$NAME" >/dev/null
printf '%s\n' "$run_exit" > "$(dirname "$CONSOLE")/refresh.exit"
exit "$run_exit"
