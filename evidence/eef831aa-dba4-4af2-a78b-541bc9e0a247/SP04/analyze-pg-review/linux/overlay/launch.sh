#!/bin/bash
# One Linux arm64 Rust 1.94 builder after the pre-overlay smoke container is gone.
# The archive stays the fixed commit. This script copies only the review test.
# Origin: a241 overlay launch 18b9c80e85d0d6442b52b764043f3a643e1b1be577ff9a06620508a901bf0a54.
# Changes: a247 names, analyze_pg_a247_review.rs only, fixture without private keys.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a247/SP04/analyze-pg-review
PROOF=${EVIDENCE}/linux/overlay
ROOT=/tmp/oo-a247-eef831aa-dba4-4af2-a78b-541bc9e0a247
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=6ac2216505723b51ae116b1938aca47846ade35f
NAME=oo-a247-proof
NET=oo-a247-eef831aa-net
CARGO_VOL=oo-a247-eef831aa-cargo
TARGET_VOL=oo-a247-eef831aa-target
FIXTURE=${ROOT}/fixture
OVERLAY=${EVIDENCE}/overlay/analyze_pg_a247_review.rs
DEST=${ROOT}/archive/crates/oo-gateway/tests/analyze_pg_a247_review.rs

mkdir -p "$PROOF/phases"
chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite overlay console.log" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -f "$EVIDENCE/linux/smoke/smoke.exit" ] || [ "$(cat "$EVIDENCE/linux/smoke/smoke.exit")" != "0" ]; then
  echo "pre-overlay smoke has not passed" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -f "$OVERLAY" ]; then
  echo "review test source is missing" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -d "$ROOT/archive/crates/oo-gateway" ]; then
  echo "fixed archive is missing" >&2
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
if ! docker ps --filter name=^/oo-a247-pg$ --filter status=running --format '{{.Names}}' | grep -qx 'oo-a247-pg'; then
  echo "private postgres is not running" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -f "$FIXTURE/ca.crt" ] || [ ! -f "$FIXTURE/admin.pass" ] || [ ! -f "$FIXTURE/wrong-ca.crt" ]; then
  echo "fixture secret files are missing" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ -e "$FIXTURE/server.key" ] || [ -e "$FIXTURE/ca.key" ] || [ -e "$FIXTURE/wrong.key" ] || [ -e "$FIXTURE/server.csr" ]; then
  echo "fixture directory contains a private key" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
smoke_builder=$(docker ps -q --filter name=^/oo-a247-smoke$ --filter status=running)
if [ -n "$smoke_builder" ]; then
  echo "smoke builder is still running" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a247 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
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

cp "$OVERLAY" "$DEST"
git -C "$SRC" hash-object "$OVERLAY" > "$PROOF/overlay-source.blob"
git -C "$SRC" hash-object "$DEST" > "$PROOF/overlay-archive.blob"
if [ "$(cat "$PROOF/overlay-source.blob")" != "$(cat "$PROOF/overlay-archive.blob")" ]; then
  echo "review test copy mismatch" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
blob_ok=0
{
  while IFS='|' read -r path; do
    [ -n "$path" ] || continue
    want=$(git -C "$SRC" rev-parse "$FIXED:$path")
    got=$(git -C "$SRC" hash-object "$ROOT/archive/$path")
    printf '%s want=%s got=%s\n' "$path" "$want" "$got"
    if [ "$want" != "$got" ]; then
      blob_ok=1
    fi
  done <<'BLOBS'
crates/oo-pipeline/src/bin/analyze-pg.rs
crates/oo-pipeline/tests/analyze_pg_cli.rs
contracts/pipeline/analyze-pg-cli-v1.md
crates/oo-pipeline/src/bin/init-pg.rs
crates/oo-pipeline/src/bin/oo-pipeline.rs
crates/oo-pipeline/src/target_statistics.rs
crates/oo-pipeline/src/import_cli.rs
Cargo.lock
BLOBS
} > "$PROOF/archive-overlay.txt"
if [ "$blob_ok" -ne 0 ]; then
  echo "product blob changed before the overlay run" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi

cat > "$PROOF/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a247
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
memory=6g
cpus=1
global_cap=10
archive=${ROOT}/archive
fixed=${FIXED}
live_env=OO_A247_LIVE
fixture_mount=/run/oo-a247/fixture
overlay=analyze_pg_a247_review.rs
postgres=oo-a247-pg
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
  -e OO_A247_LIVE=1 \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${FIXTURE},target=/run/oo-a247/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a247 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a247 \
  --label oo.task=analyze-pg-review-overlay \
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
docker rm "$NAME" > "$PROOF/container-removed.txt"
exit "$run_exit"
