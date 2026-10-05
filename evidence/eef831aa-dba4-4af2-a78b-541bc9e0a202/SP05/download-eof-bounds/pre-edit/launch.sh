#!/bin/bash
# Pre-edit Linux 1.94 smoke of the committed file-hosting tree, then the a194 reproduction.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a202/SP05/download-eof-bounds/pre-edit
ROOT=/tmp/oo-a202-eef831aa-dba4-4af2-a78b-541bc9e0a202
FIXED=e3363ffecc21af4dc9fcf75509978da34b0860b1
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a202-pre-edit
NET=oo-a202-eef831aa-net
CARGO_VOL=oo-a202-eef831aa-cargo
TARGET_VOL=oo-a202-eef831aa-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a202
SNAP=${ROOT}/pre-edit-archive

mkdir -p "$EVIDENCE/phases" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite pre-edit console.log" >&2
  exit 2
fi
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

cleanup() {
  docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt" 2>/dev/null || true
  docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} readonly_root={{.HostConfig.ReadonlyRootfs}} memory={{.HostConfig.Memory}} memoryswap={{.HostConfig.MemorySwap}}' > "$EVIDENCE/hostconfig.txt" 2>/dev/null || true
  docker inspect "$NAME" --format '{{json .HostConfig.Binds}}{{json .HostConfig.Mounts}}' > "$EVIDENCE/mounts.txt" 2>/dev/null || true
  docker rm -f "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

{
  echo "fixed=$FIXED"
  echo "workspace_head=$(git -C "$SRC" rev-parse HEAD)"
  echo "fixed_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")"
  echo "head_tree=$(git -C "$SRC" rev-parse "HEAD^{tree}")"
} > "$EVIDENCE/archive-base.txt"

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  docker ps --format 'name={{.Names}} image={{.Image}} status={{.Status}}'
  echo "labeled:"
  docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}'
  echo "host_toolchain_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver|rustup)$/ {print}' || true
} > "$EVIDENCE/builders-before.txt"

running_rust=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
running_image=$(docker ps --filter ancestor="$RUST_IMAGE" --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
if [ "$running_image" -gt "$running_rust" ]; then
  running_rust=$running_image
fi
own=$(docker ps -q --filter label=oo.owner=a202 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
printf 'running_rust_builders=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 6 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive HEAD | tar -x -C "$SNAP"
git -C "$SRC" rev-parse "HEAD^{tree}" > "$EVIDENCE/archive-tree.txt"
if [ -f "$SNAP/crates/oo-control/tests/repro_download_eof.rs" ]; then
  echo "HEAD archive already contains the reproduction overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

{
  ok=1
  for path in \
    crates/oo-control/src/file_hosting.rs \
    crates/oo-control/tests/file_hosting.rs \
    crates/oo-control/src/lib.rs \
    crates/oo-control/Cargo.toml \
    contracts/control/local-file-hosting-v1.md \
    Cargo.lock
  do
    blob=$(git -C "$SRC" rev-parse "HEAD:${path}")
    fixed_blob=$(git -C "$SRC" rev-parse "${FIXED}:${path}" 2>/dev/null || echo missing)
    snap_blob=$(git -C "$SNAP" hash-object "$path")
    sha=$(shasum -a 256 "$SNAP/$path" | awk '{print $1}')
    match=0
    if [ "$blob" = "$snap_blob" ]; then
      match=1
    else
      ok=0
    fi
    same_fixed=0
    if [ "$blob" = "$fixed_blob" ]; then
      same_fixed=1
    fi
    echo "path=$path blob=$blob snap=$snap_blob fixed_blob=$fixed_blob sha256=$sha match=$match same_as_fixed=$same_fixed"
  done
  echo "archive_hashes_match=$ok"
  if [ "$ok" != 1 ]; then
    echo "archive product bytes differ from HEAD" >&2
    printf '%s\n' 5 > "$EVIDENCE/docker-run.exit"
    exit 5
  fi
} > "$EVIDENCE/archive-match.txt"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$RUST_IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
grep -q 'os=linux' "$EVIDENCE/image.txt"
grep -q 'sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5' "$EVIDENCE/image.txt"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a202 \
    --label oo.session="$SESSION" \
    --label oo.kind=network \
    "$NET" > "$EVIDENCE/network-create.txt"
fi
docker network inspect "$NET" --format 'name={{.Name}} driver={{.Driver}} internal={{.Internal}}' > "$EVIDENCE/network.txt"
grep -q 'internal=false' "$EVIDENCE/network.txt"

if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a202 \
    --label oo.session="$SESSION" \
    --label oo.kind=cargo-home \
    "$CARGO_VOL" > "$EVIDENCE/volume-cargo-create.txt"
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a202 \
    --label oo.session="$SESSION" \
    --label oo.kind=cargo-target \
    "$TARGET_VOL" > "$EVIDENCE/volume-target-create.txt"
fi
docker volume inspect "$CARGO_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-cargo.txt"
docker volume inspect "$TARGET_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-target.txt"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named pre-edit container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
head=$(git -C "$SRC" rev-parse HEAD)
tree=$(git -C "$SRC" rev-parse "HEAD^{tree}")
fixed=${FIXED}
composition=git archive of HEAD; reproduction overlay copied only after smoke
image=${RUST_IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
postgres=none
tmpfs=/tmp:rw,nosuid,nodev,size=2g
memory=8g
phases=fmt-check fetch lock-unchanged test clippy repro
package=oo-control
hash_oracle=shasum and git hash-object; no python hashlib
EOF

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
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=2g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${SNAP},target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a202 \
  --label oo.session="$SESSION" \
  --label oo.task=download-eof-bounds-pre-edit \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"

{
  ok=1
  for path in \
    crates/oo-control/src/file_hosting.rs \
    crates/oo-control/tests/file_hosting.rs \
    crates/oo-control/src/lib.rs \
    crates/oo-control/Cargo.toml \
    contracts/control/local-file-hosting-v1.md \
    Cargo.lock
  do
    blob=$(git -C "$SRC" rev-parse "HEAD:${path}")
    snap_blob=$(git -C "$SNAP" hash-object "$path")
    match=0
    if [ "$blob" = "$snap_blob" ]; then
      match=1
    else
      ok=0
    fi
    echo "path=$path blob=$blob snap=$snap_blob match=$match"
  done
  if [ -f "$SNAP/crates/oo-control/tests/repro_download_eof.rs" ]; then
    echo "repro_overlay=present"
  else
    echo "repro_overlay=absent"
    ok=0
  fi
  echo "product_unchanged=$ok"
} > "$EVIDENCE/archive-after.txt"

echo "docker_run_exit=$run_exit"
exit "$run_exit"
