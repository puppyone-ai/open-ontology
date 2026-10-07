#!/bin/bash
# One Linux arm64 Rust 1.94 builder and one private PostgreSQL 17.10.
# Fixed archive 96afe70, then an overlay of tools/ontology-dialogue only.
# Launch shape follows this task's smoke launch and a200's proof launch.
# Memory is 6g and CPUs are 1. The smoke logs are not rewritten.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a300/SP11/ontology-dialogue
PROOF=${EVIDENCE}/linux/proof
ROOT=/tmp/oo-a300-eef831aa-dba4-4af2-a78b-541bc9e0a300
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PGIMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
FIXED=96afe70fd68a56ae8eec7504bf20e09ecd3cdf65
NAME=oo-a300-proof
PGNAME=oo-a300-pg
NET=oo-a300-eef831aa-net
CARGO_VOL=oo-a300-eef831aa-cargo
TARGET_VOL=oo-a300-eef831aa-target
A198=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a198/SP06/query-pg-cli
A261=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a261/SP11/pg-onboarding-cli

mkdir -p "$PROOF/phases" "$PROOF/live" "$ROOT"
printf '%s\n' "$$" > "$PROOF/launch.pid"
chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh" "$EVIDENCE/pg/up.sh" "$EVIDENCE/pg/pg-wrap.sh" "$EVIDENCE/linux/libclang-extract.sh"

fail() {
  printf '%s\n' "$1" > "$PROOF/docker-run.exit"
  echo "$2" >&2
  exit "$1"
}

if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
if [ -f "$EVIDENCE/linux/smoke/console.log" ] && [ ! -f "$EVIDENCE/linux/smoke/launch.exit" ]; then
  fail 2 "smoke launch.exit is missing"
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  fail 1 "pinned rust image is not present locally; not pulling"
fi
if ! docker image inspect "$PGIMAGE" >/dev/null 2>&1; then
  fail 1 "pinned postgres image is not present locally; not pulling"
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PROOF/image.txt"
grep -q 'arch=arm64' "$PROOF/image.txt" || fail 1 "rust image is not arm64"
docker image inspect "$PGIMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PROOF/postgres-image.txt"
grep -q 'arch=arm64' "$PROOF/postgres-image.txt" || fail 1 "postgres image is not arm64"

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
  fail 3 "deferred: global rust builder capacity is full"
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a300 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  fail 3 "deferred: this task already has a rust builder"
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  fail 2 "named proof container already exists"
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  fail 2 "task network is missing"
fi

hash_file() {
  shasum -a 256 "$1" | awk '{print $1}'
}
{
  echo "fixed=${FIXED}"
  git -C "$SRC" rev-parse "${FIXED}^{commit}"
  echo "smoke_launch=${EVIDENCE}/linux/smoke/launch.sh"
  echo "smoke_launch_sha256=$(hash_file "$EVIDENCE/linux/smoke/launch.sh")"
  echo "a200_launch=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  echo "a200_launch_sha256=$(hash_file "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh")"
  echo "libclang_copy=${EVIDENCE}/linux/libclang-extract.sh"
  echo "libclang_copy_sha256=$(hash_file "$EVIDENCE/linux/libclang-extract.sh")"
  echo "libclang_origin=${A198}/linux/libclang-extract.sh"
  echo "libclang_origin_sha256=$(hash_file "$A198/linux/libclang-extract.sh")"
  echo "pg_wrap_copy=${EVIDENCE}/pg/pg-wrap.sh"
  echo "pg_wrap_copy_sha256=$(hash_file "$EVIDENCE/pg/pg-wrap.sh")"
  echo "pg_wrap_origin=${A198}/pg/pg-wrap.sh"
  echo "pg_wrap_origin_sha256=$(hash_file "$A198/pg/pg-wrap.sh")"
  echo "pg_up_origin=${A198}/pg/up.sh"
  echo "pg_up_origin_sha256=$(hash_file "$A198/pg/up.sh")"
  echo "pg_up_copy=${EVIDENCE}/pg/up.sh"
  echo "pg_up_copy_sha256=$(hash_file "$EVIDENCE/pg/up.sh")"
  echo "sha256_manifest_copy=${EVIDENCE}/tools/sha256_manifest.rs"
  echo "sha256_manifest_copy_sha256=$(hash_file "$EVIDENCE/tools/sha256_manifest.rs")"
  echo "sha256_manifest_a261=${A261}/tools/sha256_manifest.rs"
  echo "sha256_manifest_a261_sha256=$(hash_file "$A261/tools/sha256_manifest.rs")"
  echo "changes=owner a300; SAN DNS:oo-a300-pg; public fixture without private keys; builder 6g/1cpu; writable fixed archive plus tools/ontology-dialogue overlay only"
} > "$PROOF/recipe-origins.txt"

if [ -d "$SRC/tools/ontology-dialogue/target" ]; then
  fail 2 "refusing to overlay a target directory"
fi
if [ ! -f "$SRC/tools/ontology-dialogue/Cargo.lock" ] || [ ! -f "$SRC/tools/ontology-dialogue/src/lib.rs" ]; then
  fail 2 "working tree overlay is incomplete"
fi

PROOF_SRC=${ROOT}/proof-src
rm -rf "$PROOF_SRC"
mkdir -p "$PROOF_SRC"
set -o pipefail
git -C "$SRC" archive "$FIXED" | tar -x -C "$PROOF_SRC"
archive_status=$?
set +o pipefail
if [ "$archive_status" -ne 0 ]; then
  fail 2 "git archive failed"
fi
if [ -e "$PROOF_SRC/tools/ontology-dialogue" ]; then
  fail 2 "fixed archive contains ontology-dialogue"
fi
mkdir -p "$PROOF_SRC/tools"
cp -R "$SRC/tools/ontology-dialogue" "$PROOF_SRC/tools/ontology-dialogue"
if [ ! -f "$PROOF_SRC/crates/oo-gateway/src/bin/query-pg.rs" ]; then
  fail 2 "fixed archive is missing query-pg"
fi
if [ ! -f "$PROOF_SRC/tools/ontology-dialogue/Cargo.lock" ]; then
  fail 2 "overlay is missing Cargo.lock"
fi
printf '%s\n' "$FIXED" > "$PROOF/fixed-base.txt"
git -C "$SRC" rev-parse "${FIXED}^{commit}" > "$PROOF/archive-commit.txt"
printf '%s\n' "$PROOF_SRC" > "$PROOF/proof-src.txt"

if docker ps --filter "name=^/${PGNAME}$" --filter status=running --format '{{.Names}}' | grep -qx "$PGNAME"; then
  echo "reusing ${PGNAME}" > "$PROOF/postgres-reuse.txt"
else
  if docker ps -a --filter "name=^/${PGNAME}$" --format '{{.Names}}' | grep -qx "$PGNAME"; then
    fail 2 "postgres container exists and is not running"
  fi
  set +e
  bash "$EVIDENCE/pg/up.sh" > "$PROOF/postgres-up.stdout" 2> "$PROOF/postgres-up.stderr"
  pg_up=$?
  set -u
  printf '%s\n' "$pg_up" > "$PROOF/postgres-up.exit"
  if [ "$pg_up" -ne 0 ]; then
    fail "$pg_up" "postgres did not start"
  fi
fi
PUB=$(cat "$EVIDENCE/pg/fixture-dir.txt")
if [ ! -f "$PUB/ca.crt" ] || [ ! -f "$PUB/admin.pass" ] || [ ! -f "$PUB/login.pass" ]; then
  fail 2 "public fixture is incomplete"
fi
if [ -e "$PUB/ca.key" ] || [ -e "$PUB/server.key" ] || [ -e "$PUB/server.crt" ]; then
  fail 2 "public fixture contains a private key or server certificate"
fi
printf '%s\n' "$PUB" > "$PROOF/fixture-dir.txt"

cat > "$PROOF/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a300
image=${IMAGE}
postgres_image=${PGIMAGE}
platform=linux/arm64
toolchain=1.94.0
network=${NET}
container=${NAME}
postgres=${PGNAME}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
memory=6g
cpus=1
pids_limit=256
phase=proof
fixed=${FIXED}
proof_src=${PROOF_SRC}
overlay=tools/ontology-dialogue
fixture=${PUB}
live=OO_A300_LIVE
EOF

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --network "$NET" \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 256 \
  --label oo.project=open-ontology \
  --label oo.owner=a300 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a300 \
  --label oo.kind=rust-builder \
  --label oo.task=ontology-dialogue-proof \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${PROOF_SRC},target=/src" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${PUB},target=/run/oo-a300/fixture,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  "$IMAGE" \
  bash /evidence/linux/proof/inside.sh > "$PROOF/console.log" 2>&1
run_exit=$?
set -u
printf '%s\n' "$run_exit" > "$PROOF/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$PROOF/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$PROOF/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$PROOF/container-removed.txt" 2>&1 || true

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PROOF/host-cargo-after.txt" || true
host_after=$(grep -c . "$PROOF/host-cargo-after.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$PROOF/builders-after.txt" || true
builder_after=$(grep -c . "$PROOF/builders-after.txt" || true)
global_after=$((host_after + builder_after))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_after" "$builder_after" "$global_after" > "$PROOF/capacity-after.txt"
if [ "$global_after" -ge 10 ]; then
  echo "host manifest deferred: capacity" > "$PROOF/manifest-host.txt"
else
  if [ -f "$PROOF/MANIFEST.sha256" ]; then
    cp "$PROOF/MANIFEST.sha256" "$PROOF/MANIFEST.in-container.sha256"
  fi
  rustc --edition 2021 -O -o /tmp/oo-a300-sha256-manifest "$EVIDENCE/tools/sha256_manifest.rs"
  /tmp/oo-a300-sha256-manifest "$EVIDENCE" "$PROOF/MANIFEST.sha256" "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a300/SP11/ontology-dialogue" \
    > "$PROOF/manifest-host.txt"
fi
exit "$run_exit"
