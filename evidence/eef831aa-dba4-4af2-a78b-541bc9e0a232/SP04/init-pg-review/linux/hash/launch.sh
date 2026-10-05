#!/bin/bash
# Compile the std-only manifest tool and hash this review directory twice.
# The second hash is the inventory. Its tool stdout is copied after that walk.
# The seal SHA256SUMS.sha256 is written after the second walk. Does not prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a232/SP04/init-pg-review
EVIDENCE=${REVIEW}/linux/hash
OUT=/tmp/oo-a232-eef831aa-dba4-4af2-a78b-541bc9e0a232-hash
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a232-hash
REFRESH=oo-a232-hash-refresh

chmod 755 "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite hash console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
mkdir -p "$OUT"
own=$(docker ps -q --filter label=oo.owner=a232 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1 || docker container inspect "$REFRESH" >/dev/null 2>&1; then
  echo "named hash container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network none --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 rustc --edition 2024 -D warnings tools/sha256_manifest.rs" > "$EVIDENCE/invocation.txt"
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
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --mount "type=bind,source=${OUT},target=/out" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --label oo.project=open-ontology \
  --label oo.owner=a232 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a232 \
  --label oo.task=init-pg-review-hash \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/SHA256SUMS' \
  > "$OUT/first-console.txt" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  {
    echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
    echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
    echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
    echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
    echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
    echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
    echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
    echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
    echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
  } > "$EVIDENCE/hostconfig.txt"
  docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
else
  echo "container was not created" > "$EVIDENCE/hostconfig.txt"
  printf '%s\n' missing > "$EVIDENCE/container-removed.txt"
fi
cp "$OUT/first-console.txt" "$EVIDENCE/console.log"
if [ "$run_exit" -ne 0 ]; then
  exit "$run_exit"
fi
own=$(docker ps -q --filter label=oo.owner=a232 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$OUT/refresh-capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: refresh builder capacity" >&2
  printf '%s\n' 3 > "$OUT/refresh.exit"
  exit 3
fi
set +e
docker run --name "$REFRESH" \
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
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --mount "type=bind,source=${OUT},target=/out" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --label oo.project=open-ontology \
  --label oo.owner=a232 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a232 \
  --label oo.task=init-pg-review-hash-refresh \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash -c 'export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH; rustc --edition 2024 -D warnings /evidence/tools/sha256_manifest.rs -o /usr/local/bin/sha256_manifest && /usr/local/bin/sha256_manifest /evidence /evidence/SHA256SUMS > /out/evidence.txt && mkdir -p /seal && cp /evidence/SHA256SUMS /seal/SHA256SUMS && /usr/local/bin/sha256_manifest /seal /evidence/SHA256SUMS.sha256 > /out/seal.txt' \
  > "$OUT/refresh-console.txt" 2>&1
refresh_exit=$?
set +e
if docker container inspect "$REFRESH" >/dev/null 2>&1; then
  {
    echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$REFRESH")"
    echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$REFRESH")"
    echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$REFRESH")"
    echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$REFRESH")"
    echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$REFRESH")"
    echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$REFRESH")"
    echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$REFRESH")"
    echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$REFRESH")"
    echo "Id=$(docker inspect -f '{{.Id}}' "$REFRESH")"
  } > "$OUT/refresh-hostconfig.txt"
  docker rm "$REFRESH" > "$OUT/refresh-removed.txt"
else
  echo "container was not created" > "$OUT/refresh-hostconfig.txt"
  printf '%s\n' missing > "$OUT/refresh-removed.txt"
fi
printf '%s\n' "$refresh_exit" > "$OUT/refresh.exit"
if [ "$refresh_exit" -ne 0 ]; then
  exit "$refresh_exit"
fi
cp "$OUT/evidence.txt" "$REVIEW/linux/hash-tool.txt"
cp "$OUT/seal.txt" "$REVIEW/linux/hash-seal-stdout.txt"
cp "$OUT/refresh-console.txt" "$EVIDENCE/refresh-console.txt"
cp "$OUT/refresh-hostconfig.txt" "$EVIDENCE/refresh-hostconfig.txt"
cp "$OUT/refresh-removed.txt" "$EVIDENCE/refresh-removed.txt"
cp "$OUT/refresh.exit" "$EVIDENCE/refresh.exit"
cp "$OUT/refresh-capacity.txt" "$EVIDENCE/refresh-capacity.txt"
rm -rf "$OUT"
if [ -e "$OUT" ]; then
  printf '%s\n' present > "$EVIDENCE/outside-tmp-removed.txt"
else
  printf '%s\n' absent > "$EVIDENCE/outside-tmp-removed.txt"
fi
exit 0
