#!/bin/bash
# One owned Linux arm64 Rust 1.94 builder for the column-ACL install.
# The repo mount is read-write so cargo can rename Cargo.lock. No host home, no host port.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a171
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP06/pg-policy-compiler
SEC=/tmp/oo-a171-${RUN_ID}/secrets
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a171-rust194
NET=oo-a171-net
CARGO_VOL=oo-a171-cargo
TARGET_VOL=oo-a171-target

mkdir -p "$EVIDENCE/linux/install-phases" "$EVIDENCE/supply" "$EVIDENCE/live"
chmod 755 "$EVIDENCE/linux/install-inside.sh" "$EVIDENCE/linux/install-launch.sh"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/linux/install-host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/linux/install-host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$EVIDENCE/linux/install-builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/linux/install-builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n" | tee "$EVIDENCE/linux/install-capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/install-launch.exit"
  exit 3
fi
own=$(docker ps -a --filter label=oo.owner=a171 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  printf '%s\n' "$own" > "$EVIDENCE/linux/install-deferred-own.txt"
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/linux/install-launch.exit"
  exit 3
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/admin.pass" ] || [ ! -f "$SEC/login.pass" ]; then
  echo "fixture secrets are absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/linux/install-launch.exit"
  exit 2
fi

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/linux/install-image.txt"
grep -q 'arch=arm64' "$EVIDENCE/linux/install-image.txt"

cat > "$EVIDENCE/linux/install-invocation.txt" <<EOF
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
repo_mount=read-write-for-lockfile-rename
phase=install
EOF

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 8g \
  --cpus 3 \
  --pids-limit 2048 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,size=1g \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  --mount "type=bind,source=${REPO},target=/src" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/run/oo-a171/fixture,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a171 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=rust-builder \
  --label oo.task=pg-policy-compiler \
  --label oo.scope=sp06-pg-policy-compiler \
  "$IMAGE" \
  bash /src/evidence/${RUN_ID}/SP06/pg-policy-compiler/linux/install-inside.sh \
  > "$EVIDENCE/linux/install-docker-run.log" 2>&1
status=$?
set -e
printf '%s\n' "$status" > "$EVIDENCE/linux/install-docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$EVIDENCE/linux/install-inspect.txt" || true
exit "$status"
