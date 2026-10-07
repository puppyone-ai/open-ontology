#!/bin/bash
# Clippy component install on the private a354 cargo volume. 6GiB, 1 CPU.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/linux/smoke-clippy/launch.sh
# Change: owner a354, UUID-private network and volumes already created by the preoverlay smoke.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a354/SP07/selfbuild-rules
DIR=${EVIDENCE}/linux/smoke-clippy
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a354-smoke-clippy
NET=oo-a354-eef831aa-net
CARGO_VOL=oo-a354-eef831aa-cargo
TARGET_VOL=oo-a354-eef831aa-target
chmod 755 "$DIR/inside.sh" "$DIR/launch.sh"
mkdir -p "$DIR/phases"
if [ -f "$DIR/console.log" ]; then
  echo "refusing to overwrite smoke-clippy console.log" >&2
  exit 2
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a354 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi
ps -axo pid=,comm= | awk '$2 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/ {print}' > "$DIR/host-cargo-procs.txt" || true
host_n=$(grep -c . "$DIR/host-cargo-procs.txt" || true)
docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}' > "$DIR/builders-before.txt" || true
builder_n=$(grep -c . "$DIR/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$DIR/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
printf 'a328-smoke-clippy %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/linux/smoke-clippy/launch.sh)" > "$DIR/recipe-origins.txt"
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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${DIR}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a354 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a354 \
  --label oo.task=selfbuild-rules-clippy-component \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$DIR/console.log" 2>&1
run_status=$?
set -e
printf '%s\n' "$run_status" > "$DIR/docker-run.exit"
docker inspect --format 'oom={{.State.OOMKilled}} status={{.State.Status}} exit={{.State.ExitCode}} mem={{.HostConfig.Memory}} cpu={{.HostConfig.NanoCpus}} priv={{.HostConfig.Privileged}} net={{.HostConfig.NetworkMode}}' "$NAME" > "$DIR/hostconfig.txt"
docker rm "$NAME" >/dev/null
printf '%s\n' removed > "$DIR/container-removed.txt"
exit "$run_status"
