#!/bin/bash
# One owned Linux arm64 Rust 1.94 builder. Reuses the preedit cargo and target volumes.
# Does not pull, prune, or touch foreign containers. build01 is left in place.
set -eu
RUN_ID=a207build07
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a207/SP05/file-parser-worker/build07
NAME=oo-a207-msrv-${RUN_ID}
NET=oo-a207-a207preedit01-net
CARGO_VOL=oo-a207-a207preedit01-cargo
TARGET_VOL=oo-a207-a207preedit01-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a207
REPO=/Users/redux/puppy-one/open-ontology

mkdir -p "$EVIDENCE"
chmod +x "$EVIDENCE/inside.sh" "$EVIDENCE/lock-gate.sh"
rm -f "$EVIDENCE/docker-run.exit"

cleanup_exit() {
  status=$?
  if [ ! -f "$EVIDENCE/docker-run.exit" ]; then
    printf '%s\n' "$status" > "$EVIDENCE/docker-run.exit"
  fi
}
trap cleanup_exit EXIT

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host_cargo_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/ {print}' || true
  echo "rust_builder_labels:"
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}} {{.Status}} {{.Label "oo.owner"}}' || true
} > "$EVIDENCE/builders-before.txt"

running=$(docker ps --filter label=oo.project=open-ontology --filter label=oo.kind=rust-builder --format '{{.Names}}')
count=$(printf '%s\n' "$running" | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -ax -o comm= | awk '$1 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/' | wc -l | tr -d ' ')
printf 'docker_rust_builders=%s\nhost_cargo=%s\ncap=10\n' "$count" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$((count + host_cargo))" -ge 10 ]; then
  echo "deferred: rust builders already at global max" | tee "$EVIDENCE/launch.stderr" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a207 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" | tee "$EVIDENCE/launch.stderr" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" | tee "$EVIDENCE/launch.stderr" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null

git -C "$REPO" rev-parse HEAD > "$EVIDENCE/source-commit.txt"
git -C "$REPO" rev-parse 'HEAD^{tree}' > "$EVIDENCE/source-tree.txt"

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
source_commit=$(cat "$EVIDENCE/source-commit.txt")
source_tree=$(cat "$EVIDENCE/source-tree.txt")
fixed_base=a661dcdf72ff91561ae6b7cb79916e582884d08a
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
repo_mount=${REPO}
repo_mount_mode=read_write
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
cap_drop=ALL
cap_add=none
init=yes
no_new_privileges=yes
postgres=none
memory=8g
cpus=4
command=cargo fetch --locked --offline; lock-gate against pre-arrow; cargo test --locked --offline -p oo-connectors --test file_parser_worker; cargo clippy --locked --offline -p oo-connectors --bin oo-file-parser-worker --test file_parser_worker -- -D warnings
EOF

if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "refusing to replace an existing container ${NAME}" | tee "$EVIDENCE/launch.stderr" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi

set +e
docker run \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${REPO},target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a207 \
  --label oo.session="${SESSION}" \
  --label oo.task=file-parser-worker \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp05-w02 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt" || true
docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} readonly_root={{.HostConfig.ReadonlyRootfs}}' > "$EVIDENCE/hostconfig.txt" || true
docker inspect "$NAME" --format '{{json .HostConfig.Binds}}{{json .HostConfig.Mounts}}' > "$EVIDENCE/mounts.txt" || true
docker inspect "$IMAGE" --format '{{.Id}} {{.Os}}/{{.Architecture}}' > "$EVIDENCE/image.txt" || true
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
