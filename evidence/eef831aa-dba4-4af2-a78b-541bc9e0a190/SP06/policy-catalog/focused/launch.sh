#!/bin/bash
# One owned Linux arm64 Rust 1.94 builder for the focused catalog proof.
# Builds an overlay of the immutable fixed archive. Does not compile the moving workspace.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a190
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP06/policy-catalog
FIXED=/tmp/oo-a190-fixed
SRC=/tmp/oo-a190-overlay
SEC=/tmp/oo-a190-${RUN_ID}/secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a190-rust194
NET=oo-a190-net
CARGO_VOL=oo-a190-cargo
TARGET_VOL=oo-a190-target

mkdir -p "$EVIDENCE/focused/phases"
rm -f "$EVIDENCE/focused/launch.exit" "$EVIDENCE/focused/docker-run.exit"
chmod 755 "$EVIDENCE/focused/inside.sh" "$EVIDENCE/focused/launch.sh"

if [ ! -f "$FIXED/Cargo.lock" ] || [ ! -f "$FIXED/crates/oo-gateway/src/policy.rs" ]; then
  echo "fixed archive is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/focused/launch.exit"
  exit 2
fi
if [ -f "$FIXED/crates/oo-gateway/src/policy_catalog.rs" ]; then
  echo "fixed archive already contains policy_catalog.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/focused/launch.exit"
  exit 2
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/admin.pass" ] || [ ! -f "$SEC/login.pass" ]; then
  echo "fixture secrets are missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/focused/launch.exit"
  exit 2
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

existing=$(docker ps -a --filter "name=^/${NAME}$" --format '{{.Status}}')
if [ -n "$existing" ]; then
  case "$existing" in
    Exited*) docker rm "$NAME" > "$EVIDENCE/focused/removed-preedit.txt" ;;
    *)
      echo "deferred: rust builder is still running" >&2
      printf '%s\n' 3 > "$EVIDENCE/focused/launch.exit"
      exit 3
      ;;
  esac
fi

rm -rf "$SRC"
mkdir -p "$SRC"
cp -a "$FIXED"/. "$SRC"/
cp "$REPO/crates/oo-gateway/src/policy.rs" "$SRC/crates/oo-gateway/src/policy.rs"
cp "$REPO/crates/oo-gateway/src/policy_catalog.rs" "$SRC/crates/oo-gateway/src/policy_catalog.rs"
cp "$REPO/crates/oo-gateway/tests/policy_catalog_pg_live.rs" "$SRC/crates/oo-gateway/tests/policy_catalog_pg_live.rs"
if [ ! -f "$SRC/crates/oo-gateway/src/policy_catalog.rs" ]; then
  echo "overlay copy failed" >&2
  printf '%s\n' 2 > "$EVIDENCE/focused/launch.exit"
  exit 2
fi

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/focused/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/focused/image.txt"

cat > "$EVIDENCE/focused/invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
source_mount=${SRC}
fixed_archive=${FIXED}
source_commit=3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
phase=focused
global_cap=6
EOF

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
  --mount "type=bind,source=${SEC},target=/run/oo-a190/fixture,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a190 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=policy-catalog \
  --label oo.scope=sp06-policy-catalog \
  "$IMAGE" \
  bash /evidence/focused/inside.sh \
  > "$EVIDENCE/focused/docker-run.log" 2>&1
status=$?
set -e
printf '%s\n' "$status" > "$EVIDENCE/focused/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} capdrop={{json .HostConfig.CapDrop}}' > "$EVIDENCE/focused/inspect.txt" || true
cp "$SRC/crates/oo-gateway/src/policy_catalog.rs" "$REPO/crates/oo-gateway/src/policy_catalog.rs"
cp "$SRC/crates/oo-gateway/tests/policy_catalog_pg_live.rs" "$REPO/crates/oo-gateway/tests/policy_catalog_pg_live.rs"
(
  cd "$REPO"
  shasum -a 256 \
    crates/oo-gateway/src/policy.rs \
    crates/oo-gateway/src/policy_catalog.rs \
    crates/oo-gateway/tests/policy_catalog_pg_live.rs \
    contracts/gateway/policy-catalog-v1.md
) > "$EVIDENCE/focused/source-sha256.txt"
printf '%s\n' "$status" > "$EVIDENCE/focused/launch.exit"
exit "$status"
