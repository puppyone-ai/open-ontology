#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Writable copy of the fixed archive plus the review test.
# The smoke archive stays unchanged. PostgreSQL is the already running oo-a208-pg.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a208/SP09/query-cli-review
PROOF=${EVIDENCE}/linux/proof
ROOT=/tmp/oo-a208-eef831aa-dba4-4af2-a78b-541bc9e0a208
SEC=${ROOT}/secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=a661dcdf72ff91561ae6b7cb79916e582884d08a
NAME=oo-a208-proof
NET=oo-a208-eef831aa-net
CARGO_VOL=oo-a208-eef831aa-cargo
TARGET_VOL=oo-a208-eef831aa-target

chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh"
mkdir -p "$PROOF/phases"
if [ -f "$PROOF/docker-run.exit" ]; then
  echo "refusing to overwrite proof docker-run.exit" >&2
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/bin/query-pg.rs" ]; then
  echo "fixed archive is missing" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/query_pg_cli_a208_review.rs" ]; then
  echo "smoke archive already contains the review overlay" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -f "$EVIDENCE/overlay/query_pg_cli_a208_review.rs" ]; then
  echo "overlay source is missing" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/login.pass" ] || [ ! -f "$SEC/admin.pass" ] || [ ! -f "$SEC/other.crt" ]; then
  echo "fixture secrets are missing" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "task network is missing" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
pg_state=$(docker inspect -f '{{.State.Running}}' oo-a208-pg 2>/dev/null || echo missing)
printf '%s\n' "$pg_state" > "$PROOF/postgres-running.txt"
if [ "$pg_state" != "true" ]; then
  echo "owned postgres is not running" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PROOF/host-cargo-procs.txt" || true
host_n=$(grep -c . "$PROOF/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$PROOF/builders-before.txt" || true
builder_n=$(grep -c . "$PROOF/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=6\n' "$host_n" "$builder_n" "$global_n" > "$PROOF/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a208 --filter label=oo.kind=rust-builder)
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

rm -rf "$ROOT/proof-src"
mkdir -p "$ROOT/proof-src"
cp -a "$ROOT/archive/." "$ROOT/proof-src/"
cp "$EVIDENCE/overlay/query_pg_cli_a208_review.rs" "$ROOT/proof-src/crates/oo-gateway/tests/query_pg_cli_a208_review.rs"
if [ -f "$ROOT/archive/crates/oo-gateway/tests/query_pg_cli_a208_review.rs" ]; then
  echo "copy leaked the overlay into the smoke archive" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
printf 'proof_src=%s overlay=copied smoke_archive=unchanged\n' "$ROOT/proof-src" > "$PROOF/tree.txt"

cat > "$PROOF/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a208
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
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
postgres=oo-a208-pg
phase=proof
global_cap=6
fixed=${FIXED}
executor_review=a205_REQUEST_CHANGES_retained
EOF

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/proof-src,target=/src,readonly" \
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/run/oo-a208/fixture,readonly" \
  --mount "type=bind,source=${SEC},target=/run/oo-a190/fixture,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a208 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a208 \
  --label oo.kind=rust-builder \
  --label oo.task=query-cli-review \
  --label oo.scope=sp09-query-cli-review \
  "$IMAGE" \
  bash /inside.sh \
  > "$PROOF/docker-run.log" 2>&1
status=$?
set -e
printf '%s\n' "$status" > "$PROOF/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} capdrop={{json .HostConfig.CapDrop}}' > "$PROOF/inspect.txt" || true
docker rm "$NAME" > "$PROOF/removed.txt" 2>&1 || true
printf '%s\n' "$status" > "$PROOF/launch.exit"
exit "$status"
