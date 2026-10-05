#!/bin/bash
# One builder, after the smoke container is gone. Reproduces the old assertion on the fixed source.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/${RUN_ID:-eef831aa-dba4-4af2-a78b-541bc9e0a206}/SP06/catalog-test-alignment
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a206
EVIDENCE=${SRC}/evidence/${RUN_ID}/SP06/catalog-test-alignment
RED=${EVIDENCE}/linux/red
ROOT=/tmp/oo-a206-${RUN_ID}
SEC=${ROOT}/secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a206-red
NET=oo-a206-eef831aa-net
CARGO_VOL=oo-a206-eef831aa-cargo
TARGET_VOL=oo-a206-eef831aa-target

chmod 755 "$RED/inside.sh" "$RED/launch.sh"
mkdir -p "$RED/phases"
if [ -f "$RED/docker-run.exit" ]; then
  echo "refusing to overwrite red docker-run.exit" >&2
  exit 2
fi
if [ ! -f "$EVIDENCE/linux/smoke/smoke.exit" ] || [ "$(cat "$EVIDENCE/linux/smoke/smoke.exit")" != "0" ]; then
  echo "smoke has not passed" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi
if docker ps -q --filter name=^/oo-a206-smoke$ | grep -q .; then
  echo "smoke builder is still running" >&2
  printf '%s\n' 3 > "$RED/docker-run.exit"
  exit 3
fi
if ! docker ps --filter name=^/oo-a206-pg$ --filter status=running --format '{{.Names}}' | grep -q '^oo-a206-pg$'; then
  echo "postgres is not running" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/admin.pass" ] || [ ! -f "$SEC/login.pass" ]; then
  echo "fixture secrets are missing" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$RED/host-cargo-procs.txt" || true
host_n=$(grep -c . "$RED/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$RED/builders-before.txt" || true
builder_n=$(grep -c . "$RED/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=6\n' "$host_n" "$builder_n" "$global_n" > "$RED/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$RED/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a206 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$RED/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named red container already exists" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi

cat > "$RED/invocation.txt" <<EOF
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
phase=preedit-live
env=OO_A190_LIVE=1
source=fixed-archive
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
  -e OO_A190_LIVE=1 \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${RED}/inside.sh,target=/inside.sh,readonly" \
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
  > "$RED/docker-run.log" 2>&1
status=$?
set -e
printf '%s\n' "$status" > "$RED/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} capdrop={{json .HostConfig.CapDrop}}' > "$RED/inspect.txt" || true
docker rm "$NAME" > "$RED/removed.txt" 2>&1 || true
printf '%s\n' "$status" > "$RED/launch.exit"
exit "$status"
