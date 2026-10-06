#!/bin/bash
# One Linux arm64 Rust 1.94 builder for the independent live proof.
# Origin: a249 chain launch blob 627ef96ef74b908a1b78095dab2fe0ce9decc8b0,
# which reused a240 blob 8adb2df83d60560efb7e63f2874944dec5c4833a and a211 blob
# 42bfd4699327065104467767b6ef490624e350f5.
# This builder is 6g and 1 CPU. It starts only after smoke.exit is 0.
# No product overlay. The frozen test's host constant is reached by a network alias.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a252/SP06/mapping-reader-cli-review
EVIDENCE=${REVIEW}/linux/proof
ROOT=/tmp/oo-a252-eef831aa-dba4-4af2-a78b-541bc9e0a252
SEC=${ROOT}/secrets
BUILDER_SEC=${ROOT}/builder-secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a252-proof
NET=oo-a252-eef831aa-net
CARGO_VOL=oo-a252-eef831aa-cargo
TARGET_VOL=oo-a252-eef831aa-target
FIXED=9df074813572569950d85bbe951069b4db83facc

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
mkdir -p "$EVIDENCE/phases" "$REVIEW/live"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
if [ ! -f "$REVIEW/linux/smoke/smoke.exit" ] || [ "$(cat "$REVIEW/linux/smoke/smoke.exit")" != "0" ]; then
  echo "pre-live smoke did not exit 0" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$REVIEW/pg/up.exit" ] || [ "$(cat "$REVIEW/pg/up.exit")" != "0" ]; then
  echo "private postgres did not exit 0" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ "$(docker inspect -f '{{.State.Running}}' oo-a252-pg 2>/dev/null || true)" != "true" ]; then
  echo "private postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
for secret in admin.pass reader.pass writer.pass runtime.pass login.pass extra.pass ca.crt; do
  if [ ! -s "$SEC/$secret" ]; then
    echo "missing private secret file $secret" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
done
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/bin/provision-mapping-reader.rs" ]; then
  echo "fixed archive is missing the cli" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ "$(cat "$ROOT/archive/.fixed-base-sha")" != "$FIXED" ]; then
  echo "archive base sha is not the frozen commit" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$BUILDER_SEC"
mkdir -p "$BUILDER_SEC"
chmod 700 "$BUILDER_SEC"
for secret in admin.pass reader.pass writer.pass runtime.pass login.pass extra.pass ca.crt; do
  cp "$SEC/$secret" "$BUILDER_SEC/$secret"
done
chmod 600 "$BUILDER_SEC"/*
chmod 644 "$BUILDER_SEC/ca.crt"
if [ -e "$BUILDER_SEC/server.key" ] || [ -e "$BUILDER_SEC/ca.key" ]; then
  echo "builder secret dir contains a private key" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
find "$BUILDER_SEC" -maxdepth 1 -type f -print | awk -F/ '{print $NF}' | sort > "$EVIDENCE/builder-secret-names.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$EVIDENCE/builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$EVIDENCE/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a252 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A249_LIVE=1 \
  -e OO_A249_INIT_PG=/target/debug/init-pg \
  -e OO_A249_PIPELINE=/target/debug/oo-pipeline \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${BUILDER_SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a252 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a252 \
  --label oo.task=mapping-reader-cli-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
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
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
