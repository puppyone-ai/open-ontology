#!/bin/bash
# One builder on the fixed archive plus the three edited files. Smoke and red containers must be gone.
set -u
SRC=/Users/redux/puppy-one/open-ontology
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a206
EVIDENCE=${SRC}/evidence/${RUN_ID}/SP06/catalog-test-alignment
GREEN=${EVIDENCE}/linux/green
ROOT=/tmp/oo-a206-${RUN_ID}
SEC=${ROOT}/secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a206-green
NET=oo-a206-eef831aa-net
CARGO_VOL=oo-a206-eef831aa-cargo
TARGET_VOL=oo-a206-eef831aa-target

chmod 755 "$GREEN/inside.sh" "$GREEN/launch.sh"
mkdir -p "$GREEN/phases"
if [ -f "$GREEN/docker-run.exit" ]; then
  echo "refusing to overwrite green docker-run.exit" >&2
  exit 2
fi
if [ ! -f "$EVIDENCE/linux/red/reproduced.txt" ] || ! grep -q '^yes ' "$EVIDENCE/linux/red/reproduced.txt"; then
  echo "red assertion was not reproduced" >&2
  printf '%s\n' 2 > "$GREEN/docker-run.exit"
  exit 2
fi
if docker ps -q --filter label=oo.owner=a206 --filter label=oo.kind=rust-builder --filter status=running | grep -q .; then
  echo "a rust builder for this task is still running" >&2
  printf '%s\n' 3 > "$GREEN/docker-run.exit"
  exit 3
fi
if ! docker ps --filter name=^/oo-a206-pg$ --filter status=running --format '{{.Names}}' | grep -q '^oo-a206-pg$'; then
  echo "postgres is not running" >&2
  printf '%s\n' 2 > "$GREEN/docker-run.exit"
  exit 2
fi

rm -rf "$ROOT/green"
mkdir -p "$ROOT/green"
tar -C "$ROOT/archive" -cf - . | tar -C "$ROOT/green" -xf -
cp "$SRC/crates/oo-gateway/src/policy.rs" "$ROOT/green/crates/oo-gateway/src/policy.rs"
cp "$SRC/crates/oo-gateway/tests/policy_catalog_pg_live.rs" "$ROOT/green/crates/oo-gateway/tests/policy_catalog_pg_live.rs"
cp "$SRC/contracts/gateway/policy-catalog-v1.md" "$ROOT/green/contracts/gateway/policy-catalog-v1.md"
cmp -s "$ROOT/archive/crates/oo-gateway/src/query_executor.rs" "$ROOT/green/crates/oo-gateway/src/query_executor.rs"
cmp -s "$ROOT/archive/crates/oo-gateway/src/policy_catalog.rs" "$ROOT/green/crates/oo-gateway/src/policy_catalog.rs"
cmp -s "$ROOT/archive/Cargo.lock" "$ROOT/green/Cargo.lock"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$GREEN/host-cargo-procs.txt" || true
host_n=$(grep -c . "$GREEN/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$GREEN/builders-before.txt" || true
builder_n=$(grep -c . "$GREEN/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=6\n' "$host_n" "$builder_n" "$global_n" > "$GREEN/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$GREEN/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named green container already exists" >&2
  printf '%s\n' 2 > "$GREEN/docker-run.exit"
  exit 2
fi

cat > "$GREEN/invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
network=${NET}
container=${NAME}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
phase=postedit
env_live=OO_A190_LIVE=1
source=archive-plus-three-files
postgres=oo-a206-pg
alias=oo-a190-pg
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
  --mount "type=bind,source=${ROOT}/green,target=/src,readonly" \
  --mount "type=bind,source=${GREEN}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/run/oo-a190/fixture,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a206 \
  --label oo.session=${RUN_ID} \
  --label oo.kind=rust-builder \
  --label oo.task=catalog-test-alignment \
  --label oo.scope=sp06-catalog-test-alignment \
  "$IMAGE" \
  bash /inside.sh \
  > "$GREEN/docker-run.log" 2>&1
status=$?
set -e
printf '%s\n' "$status" > "$GREEN/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} capdrop={{json .HostConfig.CapDrop}}' > "$GREEN/inspect.txt" || true
docker rm "$NAME" > "$GREEN/removed.txt" 2>&1 || true
printf '%s\n' "$status" > "$GREEN/launch.exit"
exit "$status"
