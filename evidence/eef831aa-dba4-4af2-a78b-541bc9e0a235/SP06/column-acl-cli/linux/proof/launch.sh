#!/bin/bash
# One Linux arm64 Rust 1.94 builder after the pre-overlay smoke container is gone.
# Immutable archive plus the three owned CLI files. 6GiB, 1 CPU. No cargo update.
# The builder mounts the fixture directory only. Private keys stay on the database.
# Origin: a218 proof launch.sh (column-acl-set/linux/proof/launch.sh).
# Changes: a235 names, three CLI files, policy.rs stays the archive blob, OO_A235_LIVE.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a235/SP06/column-acl-cli
PROOF=${EVIDENCE}/linux/proof
ROOT=/tmp/oo-a235-eef831aa-dba4-4af2-a78b-541bc9e0a235
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=a48db85fc13148874349d318affd995f7212188d
NAME=oo-a235-proof
NET=oo-a235-eef831aa-net
CARGO_VOL=oo-a235-eef831aa-cargo
TARGET_VOL=oo-a235-eef831aa-target
FIXTURE=${ROOT}/fixture

mkdir -p "$PROOF/phases"
chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -f "$EVIDENCE/linux/smoke/smoke.exit" ] || [ "$(cat "$EVIDENCE/linux/smoke/smoke.exit")" != "0" ]; then
  echo "pre-overlay smoke has not passed" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/src/bin/install-column-acl.rs" ]; then
  echo "archive contains the cli overlay" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/install_column_acl_cli.rs" ]; then
  echo "archive contains the cli test" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/contracts/gateway/install-column-acl-cli-v1.md" ]; then
  echo "archive contains the cli contract" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/rust-toolchain.toml" ]; then
  echo "archive still has rust-toolchain.toml" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi
if ! docker ps --filter name=^/oo-a235-pg$ --filter status=running --format '{{.Names}}' | grep -qx 'oo-a235-pg'; then
  echo "private postgres is not running" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -f "$FIXTURE/ca.crt" ] || [ ! -f "$FIXTURE/admin.pass" ] || [ ! -f "$FIXTURE/login.pass" ]; then
  echo "fixture secret files are missing" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ -e "$FIXTURE/server.key" ] || [ -e "$FIXTURE/ca.key" ] || [ -e "$FIXTURE/server.csr" ]; then
  echo "fixture directory contains a private key" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
smoke_builder=$(docker ps -q --filter name=^/oo-a235-smoke$ --filter status=running)
if [ -n "$smoke_builder" ]; then
  echo "smoke builder is still running" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PROOF/host-cargo-procs.txt" || true
host_n=$(grep -c . "$PROOF/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$PROOF/builders-before.txt" || true
builder_n=$(grep -c . "$PROOF/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$PROOF/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a235 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi

PROOF_SRC=${ROOT}/proof-src
rm -rf "$PROOF_SRC"
mkdir -p "$PROOF_SRC"
if ! cp -cR "$ROOT/archive/." "$PROOF_SRC/"; then
  cp -R "$ROOT/archive/." "$PROOF_SRC/"
fi
mkdir -p "$PROOF_SRC/crates/oo-gateway/src/bin" "$PROOF_SRC/crates/oo-gateway/tests" "$PROOF_SRC/contracts/gateway"
cp "$SRC/crates/oo-gateway/src/bin/install-column-acl.rs" "$PROOF_SRC/crates/oo-gateway/src/bin/install-column-acl.rs"
cp "$SRC/crates/oo-gateway/tests/install_column_acl_cli.rs" "$PROOF_SRC/crates/oo-gateway/tests/install_column_acl_cli.rs"
cp "$SRC/contracts/gateway/install-column-acl-cli-v1.md" "$PROOF_SRC/contracts/gateway/install-column-acl-cli-v1.md"
test -s "$PROOF_SRC/crates/oo-gateway/src/bin/install-column-acl.rs"
test -s "$PROOF_SRC/crates/oo-gateway/tests/install_column_acl_cli.rs"
test -s "$PROOF_SRC/contracts/gateway/install-column-acl-cli-v1.md"
if [ -f "$ROOT/archive/crates/oo-gateway/src/bin/install-column-acl.rs" ]; then
  echo "archive gained the cli overlay during the proof copy" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
frozen_same() {
  if ! cmp -s "$1" "$2"; then
    echo "proof copy changed a frozen file: $1" >&2
    printf '%s\n' 2 > "$PROOF/docker-run.exit"
    exit 2
  fi
}
frozen_same "$ROOT/archive/crates/oo-gateway/src/policy.rs" "$PROOF_SRC/crates/oo-gateway/src/policy.rs"
frozen_same "$ROOT/archive/crates/oo-gateway/src/lib.rs" "$PROOF_SRC/crates/oo-gateway/src/lib.rs"
frozen_same "$ROOT/archive/crates/oo-gateway/Cargo.toml" "$PROOF_SRC/crates/oo-gateway/Cargo.toml"
frozen_same "$ROOT/archive/Cargo.lock" "$PROOF_SRC/Cargo.lock"
frozen_same "$ROOT/archive/contracts/gateway/column-acl-set-v1.md" "$PROOF_SRC/contracts/gateway/column-acl-set-v1.md"
frozen_same "$ROOT/archive/contracts/gateway/reviewed-policy-v1.md" "$PROOF_SRC/contracts/gateway/reviewed-policy-v1.md"
frozen_same "$ROOT/archive/crates/oo-gateway/src/bin/query-pg.rs" "$PROOF_SRC/crates/oo-gateway/src/bin/query-pg.rs"
printf '%s\n' "$PROOF_SRC" > "$PROOF/proof-src.txt"
printf '%s\n' "archive_overlay_absent" > "$PROOF/archive-overlay.txt"
cat > "$PROOF/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a235
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
network=${NET}
container=${NAME}
postgres=oo-a235-pg
database=oo_a235
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
phase=proof
memory=6g
cpus=1
global_cap=10
archive=${ROOT}/archive
proof_src=${PROOF_SRC}
fixture=${FIXTURE}
overlay=install-column-acl.rs install_column_acl_cli.rs install-column-acl-cli-v1.md
fixed=${FIXED}
live=OO_A235_LIVE
private_keys_in_builder=no
EOF

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
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
  -e OO_A235_LIVE=1 \
  --mount "type=bind,source=${PROOF_SRC},target=/src,readonly" \
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${FIXTURE},target=/run/oo-a235/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a235 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a235 \
  --label oo.task=column-acl-cli-proof \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROOF/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROOF/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Tmpfs=$(docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$PROOF/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$PROOF/mounts.txt"
if [ -f "$ROOT/archive/crates/oo-gateway/src/bin/install-column-acl.rs" ]; then
  echo "archive_overlay_present_after" >> "$PROOF/archive-overlay.txt"
else
  echo "archive_overlay_absent_after" >> "$PROOF/archive-overlay.txt"
fi
docker rm "$NAME" > "$PROOF/container-removed.txt"
exit "$run_exit"
