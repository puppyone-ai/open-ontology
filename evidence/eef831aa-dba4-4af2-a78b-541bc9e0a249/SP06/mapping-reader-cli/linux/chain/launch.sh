#!/bin/bash
# One Linux arm64 Rust 1.94 builder for the mapping-reader CLI.
# Origin: a240 linux/chain/launch.sh blob 8adb2df83d60560efb7e63f2874944dec5c4833a,
# which reused a211 blob 42bfd4699327065104467767b6ef490624e350f5.
# This builder is 6g and 1 CPU. It starts only after smoke.exit is 0.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a249/SP06/mapping-reader-cli
EVIDENCE=${REVIEW}/linux/chain
ROOT=/tmp/oo-a249-eef831aa-dba4-4af2-a78b-541bc9e0a249
SEC=${ROOT}/secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a249-chain
NET=oo-a249-eef831aa-net
CARGO_VOL=oo-a249-eef831aa-cargo
TARGET_VOL=oo-a249-eef831aa-target
FIXED=e0704257a1509b968a80a9ccd2f200319d5483ad

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
mkdir -p "$EVIDENCE/phases" "$ROOT/overlay/product" "$REVIEW/live"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite chain console.log" >&2
  exit 2
fi
if [ ! -f "$REVIEW/linux/smoke/smoke.exit" ] || [ "$(cat "$REVIEW/linux/smoke/smoke.exit")" != "0" ]; then
  echo "pre-overlay smoke did not exit 0" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$REVIEW/pg/up.exit" ] || [ "$(cat "$REVIEW/pg/up.exit")" != "0" ]; then
  echo "private postgres did not exit 0" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ "$(docker inspect -f '{{.State.Running}}' oo-a249-pg 2>/dev/null || true)" != "true" ]; then
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
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/mapping_reader_access.rs" ]; then
  echo "fixed archive is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -e "$ROOT/archive/crates/oo-gateway/src/bin/provision-mapping-reader.rs" ]; then
  echo "archive already contains the mapping reader cli" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp "$SRC/crates/oo-gateway/src/bin/provision-mapping-reader.rs" "$ROOT/overlay/product/provision-mapping-reader.rs"
cp "$SRC/crates/oo-gateway/tests/provision_mapping_reader_cli.rs" "$ROOT/overlay/product/provision_mapping_reader_cli.rs"
cp "$SRC/contracts/gateway/provision-mapping-reader-cli-v1.md" "$ROOT/overlay/product/provision-mapping-reader-cli-v1.md"
if ! grep -q 'provision_mapping_reader' "$ROOT/overlay/product/provision-mapping-reader.rs"; then
  echo "overlay binary does not call provision_mapping_reader" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
own=$(docker ps -q --filter label=oo.owner=a249 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named chain container already exists" >&2
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
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${ROOT}/overlay,target=/overlay" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a249 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a249 \
  --label oo.task=mapping-reader-cli \
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
