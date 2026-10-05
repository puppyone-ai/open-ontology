#!/bin/bash
# One running Rust 1.94 builder for the a203 harness. The exited pre-edit smoke container stays.
# Counts running builders only. Archive mount is read-only. No host cargo cache and no host network.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a203
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP11/pg-vertical-integration
SRC=/tmp/oo-a203-fixed
SEC=/tmp/oo-a203-${RUN_ID}/secrets
FIXED=0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a203-harness
NET=oo-a203-net
CARGO_VOL=oo-a203-cargo
TARGET_VOL=oo-a203-target

mkdir -p "$EVIDENCE/linux/harness-phases"
chmod 755 "$EVIDENCE/linux/harness-inside.sh" "$EVIDENCE/linux/harness-launch.sh"

if [ ! -f "$SRC/.fixed-base-sha" ] || [ ! -f "$SRC/Cargo.lock" ]; then
  echo "fixed archive is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/harness-launch.exit"
  exit 2
fi
if [ "$(cat "$SRC/.fixed-base-sha")" != "$FIXED" ]; then
  echo "fixed archive sha mismatch" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/harness-launch.exit"
  exit 2
fi
if [ -f "$SRC/crates/oo-gateway/src/bin/query-pg.rs" ]; then
  echo "fixed archive contains the moving query CLI" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/harness-launch.exit"
  exit 2
fi
for secret in admin.pass reader.pass runtime.pass party.pass invoice.pass ca.crt; do
  if [ ! -f "$SEC/$secret" ]; then
    echo "private secret file is missing" >&2
    printf '%s\n' 2 > "$EVIDENCE/linux/harness-launch.exit"
    exit 2
  fi
done

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/linux/harness-host-cargo.txt" || true
host_n=$(grep -c . "$EVIDENCE/linux/harness-host-cargo.txt" || true)
{
  docker ps --filter label=oo.kind=rust-builder --filter status=running --format '{{.Names}}'
  docker ps --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --filter status=running --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$EVIDENCE/linux/harness-builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/linux/harness-builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders_running=$builder_n global=$global_n cap=6" | tee "$EVIDENCE/linux/harness-capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/harness-launch.exit"
  exit 3
fi
if docker ps --filter "name=^/${NAME}$" --filter status=running --format '{{.Names}}' | grep -q .; then
  echo "deferred: harness builder is already running" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/harness-launch.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.Names}}' | grep -q .; then
  echo "harness container name already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/harness-launch.exit"
  exit 2
fi
if ! docker ps --filter "name=^/oo-a203-pg$" --filter status=running --format '{{.Names}}' | grep -q .; then
  echo "postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/harness-launch.exit"
  exit 2
fi

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/linux/harness-image.txt"
grep -q 'arch=arm64' "$EVIDENCE/linux/harness-image.txt"

cat > "$EVIDENCE/linux/harness-invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
network=${NET}
container=${NAME}
postgres=oo-a203-pg
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
source_mount=${SRC}
source_commit=${FIXED}
source_mount_mode=ro
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
phase=harness
global_cap=6
memory=8g
EOF

docker run -d --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,size=4g \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/harness \
  -e CARGO_NET_OFFLINE=true \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  --mount "type=bind,source=${SRC},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC}/ca.crt,target=/run/secrets/ca.crt,readonly" \
  --mount "type=bind,source=${SEC}/admin.pass,target=/run/secrets/admin.pass,readonly" \
  --mount "type=bind,source=${SEC}/reader.pass,target=/run/secrets/reader.pass,readonly" \
  --mount "type=bind,source=${SEC}/runtime.pass,target=/run/secrets/runtime.pass,readonly" \
  --mount "type=bind,source=${SEC}/party.pass,target=/run/secrets/party.pass,readonly" \
  --mount "type=bind,source=${SEC}/invoice.pass,target=/run/secrets/invoice.pass,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a203 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=pg-vertical-integration \
  --entrypoint bash \
  "$IMAGE" /evidence/linux/harness-inside.sh > "$EVIDENCE/linux/harness-container.id"

printf '%s\n' 0 > "$EVIDENCE/linux/harness-launch.exit"
echo "harness_started name=$NAME"
