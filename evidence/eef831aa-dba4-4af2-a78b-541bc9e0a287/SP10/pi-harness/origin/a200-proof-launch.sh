#!/bin/bash
# One Linux arm64 Rust 1.94 builder after the pre-overlay smoke container is gone.
# The git archive stays unchanged. The review file is a separate read-only mount.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review
PROOF=${EVIDENCE}/linux/proof
ROOT=/tmp/oo-a200-eef831aa-dba4-4af2-a78b-541bc9e0a200
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=78a60fdeabcada0d024fc6f9a81e0e6b184260e8
NAME=oo-a200-proof
NET=oo-a200-eef831aa-net
CARGO_VOL=oo-a200-eef831aa-cargo
TARGET_VOL=oo-a200-eef831aa-target
SECRETS=${ROOT}/secrets
OVERLAY=${EVIDENCE}/overlay/policy_catalog_a200_review.rs

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
if [ -f "$ROOT/archive/crates/oo-gateway/tests/policy_catalog_a200_review.rs" ]; then
  echo "archive contains the review overlay" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -s "$OVERLAY" ]; then
  echo "overlay file is missing" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi
if ! docker ps --filter name=^/oo-a200-pg$ --filter status=running --format '{{.Names}}' | grep -qx 'oo-a200-pg'; then
  echo "private postgres is not running" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if [ ! -f "$SECRETS/ca.crt" ] || [ ! -f "$SECRETS/admin.pass" ] || [ ! -f "$SECRETS/login.pass" ]; then
  echo "fixture secret files are missing" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
smoke_builder=$(docker ps -q --filter name=^/oo-a200-smoke$ --filter status=running)
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
printf 'host_cargo=%s rust_builders=%s global=%s cap=6\n' "$host_n" "$builder_n" "$global_n" > "$PROOF/capacity.txt"
if [ "$global_n" -ge 6 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a200 --filter label=oo.kind=rust-builder)
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

printf '%s\n' "archive_overlay_absent" > "$PROOF/archive-overlay.txt"
PROOF_SRC=${ROOT}/proof-src
if [ ! -f "$PROOF_SRC/crates/oo-gateway/src/policy_catalog.rs" ]; then
  rm -rf "$PROOF_SRC"
  mkdir -p "$PROOF_SRC"
  if ! cp -cR "$ROOT/archive/." "$PROOF_SRC/"; then
    cp -R "$ROOT/archive/." "$PROOF_SRC/"
  fi
fi
cp "$OVERLAY" "$PROOF_SRC/crates/oo-gateway/tests/policy_catalog_a200_review.rs"
if [ -f "$ROOT/archive/crates/oo-gateway/tests/policy_catalog_a200_review.rs" ]; then
  echo "archive gained the review overlay during the proof copy" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
printf '%s\n' "$PROOF_SRC" > "$PROOF/proof-src.txt"
cat > "$PROOF/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a200
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
network=${NET}
container=${NAME}
postgres=oo-a200-pg
database=oo_a200
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
phase=proof
global_cap=6
archive=${ROOT}/archive
proof_src=${PROOF_SRC}
overlay=copied-into-proof-src-only
fixed=${FIXED}
live=OO_A200_LIVE
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
  -e OO_A200_LIVE=1 \
  --mount "type=bind,source=${PROOF_SRC},target=/src,readonly" \
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SECRETS},target=/run/oo-a200/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a200 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a200 \
  --label oo.task=policy-catalog-review-proof \
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
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Tmpfs=$(docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$PROOF/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$PROOF/mounts.txt"
if [ -f "$ROOT/archive/crates/oo-gateway/tests/policy_catalog_a200_review.rs" ]; then
  echo "archive_overlay_present_after" >> "$PROOF/archive-overlay.txt"
else
  echo "archive_overlay_absent_after" >> "$PROOF/archive-overlay.txt"
fi
docker rm "$NAME" > "$PROOF/container-removed.txt"
exit "$run_exit"
