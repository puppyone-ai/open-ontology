#!/bin/bash
# Compile the copied Rust manifest tool and hash this evidence directory.
# No pull, no postgres, no network, no product mount.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a307/SP11/session-release-review
RUN=${EVIDENCE}/linux/manifest
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a307-manifest
VOL=oo-a307-eef831aa-hashbin

if [ -f "$RUN/console.log" ]; then
  echo "refusing to overwrite manifest console.log" >&2
  exit 2
fi
own=$(docker ps -q --filter label=oo.owner=a307 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$RUN/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$RUN/docker-run.exit"
  exit 3
fi
docker volume inspect "$VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a307 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a307 \
  --label oo.kind=hash-bin \
  "$VOL" >/dev/null
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$RUN/image.txt"
set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --network none \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 256 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${VOL},target=/out" \
  --label oo.project=open-ontology \
  --label oo.owner=a307 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a307 \
  --label oo.task=session-release-review-manifest \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'rustc --edition 2024 -C debuginfo=0 -o /out/sha256_manifest /evidence/tools/sha256_manifest.rs && /out/sha256_manifest /evidence /evidence/MANIFEST.sha256' \
  > "$RUN/console.log" 2>&1
run_exit=$?
printf '%s\n' "$run_exit" > "$RUN/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$RUN/hostconfig.txt"
docker rm "$NAME" > "$RUN/container-removed.txt"
exit "$run_exit"
