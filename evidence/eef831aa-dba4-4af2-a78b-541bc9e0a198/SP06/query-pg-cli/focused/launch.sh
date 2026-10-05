#!/bin/bash
# One owned Linux arm64 Rust 1.94 builder for the query-pg focused proof.
# Mounts the frozen 78a60fde archive plus the owned overlay. No host cargo cache.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a198
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP06/query-pg-cli
SRC=/tmp/oo-a198-fixed
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a198-rust194
NET=oo-a198-net
CARGO_VOL=oo-a198-cargo
TARGET_VOL=oo-a198-target
SEC=/tmp/oo-a198-${RUN_ID}/secrets
FIX=/tmp/oo-a198-${RUN_ID}/fixture-in
BASE=78a60fdeabcada0d024fc6f9a81e0e6b184260e8

mkdir -p "$EVIDENCE/focused/phases"
chmod 755 "$EVIDENCE/focused/launch.sh" "$EVIDENCE/focused/inside.sh"

if [ ! -f "$SRC/Cargo.lock" ] || [ ! -f "$SRC/crates/oo-gateway/src/policy_catalog.rs" ]; then
  echo "fixed archive is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/focused/launch.exit"
  exit 2
fi
if ! git -C "$REPO" diff --quiet "$BASE" -- Cargo.lock; then
  echo "workspace Cargo.lock differs from the frozen baseline; refusing" >&2
  git -C "$REPO" diff --stat "$BASE" -- Cargo.lock > "$EVIDENCE/focused/lock-workspace-diff.txt"
  printf '%s\n' 4 > "$EVIDENCE/focused/launch.exit"
  exit 4
fi

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/focused/host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/focused/host-cargo-procs.txt" || true)
{
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$EVIDENCE/focused/builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/focused/builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n cap=6" | tee "$EVIDENCE/focused/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/focused/launch.exit"
  exit 3
fi

if docker ps -a --filter name=^/${NAME}$ --filter status=exited --format '{{.ID}}' | grep -q .; then
  docker rm "$NAME" > "$EVIDENCE/focused/removed-smoke-container.txt"
fi
own=$(docker ps -a --filter label=oo.owner=a198 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  printf '%s\n' "$own" > "$EVIDENCE/focused/deferred-own.txt"
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/focused/launch.exit"
  exit 3
fi
if ! docker ps --filter name=^/oo-a198-pg$ --filter status=running --format '{{.Names}}' | grep -q .; then
  echo "private postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/focused/launch.exit"
  exit 2
fi

mkdir -p "$SRC/crates/oo-gateway/src/bin" "$SRC/crates/oo-gateway/tests" "$SRC/contracts/gateway"
cp "$REPO/crates/oo-gateway/Cargo.toml" "$SRC/crates/oo-gateway/Cargo.toml"
cp "$REPO/crates/oo-gateway/src/bin/query-pg.rs" "$SRC/crates/oo-gateway/src/bin/query-pg.rs"
cp "$REPO/crates/oo-gateway/tests/query_pg_cli.rs" "$SRC/crates/oo-gateway/tests/query_pg_cli.rs"
cp "$REPO/contracts/gateway/query-pg-cli-v1.md" "$SRC/contracts/gateway/query-pg-cli-v1.md"
if [ ! -f "$EVIDENCE/focused/Cargo.lock.baseline" ]; then
  cp "$SRC/Cargo.lock" "$EVIDENCE/focused/Cargo.lock.baseline"
fi

mkdir -p "$FIX"
cp "$SEC/admin.pass" "$SEC/login.pass" "$SEC/ca.crt" "$SEC/other.crt" "$FIX/"
chmod 644 "$FIX"/*

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/focused/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/focused/image.txt"

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  --mount "type=bind,source=${SRC},target=/src" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${FIX},target=/run/oo-a198/fixture,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a198 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=query-pg-cli \
  "$IMAGE" \
  bash /evidence/focused/inside.sh > "$EVIDENCE/focused/docker-run.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/focused/launch.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$EVIDENCE/focused/inspect.txt" || true
exit "$run_exit"
