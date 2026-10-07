#!/bin/bash
# Post-edit Linux builder. Fixed archive plus the four owned Rust files.
# Origin: a327 smoke launch. Same image, volumes, 6GiB, 1 CPU.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a327/SP06/numeric-query
POST=${EVIDENCE}/linux/postedit
ROOT=/tmp/oo-a327-eef831aa-dba4-4af2-a78b-541bc9e0a327
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=e6ebf93b5ac261c097a24d145611e0300e4f88b5
NAME=oo-a327-postedit
NET=oo-a327-eef831aa-net
CARGO_VOL=oo-a327-eef831aa-cargo
TARGET_VOL=oo-a327-eef831aa-target

chmod 755 "$POST/inside.sh" "$POST/launch.sh"
mkdir -p "$POST/phases"
if [ -f "$POST/console.log" ]; then
  echo "refusing to overwrite postedit console.log" >&2
  printf '%s\n' 2 > "$POST/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/query_executor.rs" ]; then
  echo "pre-edit archive is missing" >&2
  printf '%s\n' 2 > "$POST/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/a327_numeric.rs" ]; then
  echo "archive already contains the numeric test" >&2
  printf '%s\n' 2 > "$POST/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/work"
mkdir -p "$ROOT/work"
cp -a "$ROOT/archive/." "$ROOT/work/"
cp "$SRC/crates/oo-gateway/src/reviewed_policy.rs" "$ROOT/work/crates/oo-gateway/src/reviewed_policy.rs"
cp "$SRC/crates/oo-gateway/src/query_executor.rs" "$ROOT/work/crates/oo-gateway/src/query_executor.rs"
cp "$SRC/crates/oo-gateway/src/bin/query-pg.rs" "$ROOT/work/crates/oo-gateway/src/bin/query-pg.rs"
cp "$SRC/crates/oo-gateway/tests/a327_numeric.rs" "$ROOT/work/crates/oo-gateway/tests/a327_numeric.rs"
if ! grep -q 'Cell::Decimal' "$ROOT/work/crates/oo-gateway/src/query_executor.rs"; then
  echo "overlay copy is missing Cell::Decimal" >&2
  printf '%s\n' 2 > "$POST/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$POST/docker-run.exit"
  exit 1
fi

ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}' > "$POST/host-cargo-count.txt"
host_n=$(cat "$POST/host-cargo-count.txt")
docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$POST/builders-before.txt" || true
builder_n=$(grep -c . "$POST/builders-before.txt" || true)
printf 'host_cargo=%s rust_builders=%s cap_global=10 cap_own=1\n' "$host_n" "$builder_n" > "$POST/capacity.txt"
if [ "$builder_n" -ge 10 ] || [ "$host_n" -ge 1 ]; then
  echo "deferred: rust builder capacity host=$host_n builders=$builder_n" >&2
  printf '%s\n' 3 > "$POST/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter status=running --filter label=oo.owner=a327 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$POST/docker-run.exit"
  exit 3
fi

printf '%s\n' "$FIXED" > "$POST/fixed-commit.txt"
cat > "$POST/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a327
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
memory=6g
cpus=1
privileged=no
network_mode=bridge_not_host
pull=never
phase=postedit
archive=${ROOT}/archive
fixed=${FIXED}
EOF

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
  -e CARGO_TARGET_DIR=/target/a327 \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/work,target=/src,readonly" \
  --mount "type=bind,source=${POST}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools/sha256.rs,target=/tools/sha256.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a327 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a327 \
  --label oo.task=numeric-query-postedit \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$POST/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$POST/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
} > "$POST/hostconfig.txt"
docker rm "$NAME" >/dev/null 2>&1 || true
exit "$run_exit"
