#!/bin/bash
# Post-edit Linux 1.94 focused smoke of the file-hosting overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a186/SP05/local-hosting-bounds/post-edit
ROOT=/tmp/oo-a186-eef831aa-dba4-4af2-a78b-541bc9e0a186
FIXED=755fb84acc7f80e5ba2fba1c2b80412dab7fab26
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a186-post-edit
NET=oo-a186-eef831aa-net
CARGO_VOL=oo-a186-eef831aa-cargo
TARGET_VOL=oo-a186-eef831aa-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a186
SNAP=${ROOT}/post-edit-src

mkdir -p "$EVIDENCE/phases" "$EVIDENCE/metrics" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite post-edit console.log" >&2
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
  for path in \
    crates/oo-control/src/file_hosting.rs \
    crates/oo-control/tests/file_hosting.rs \
    contracts/control/local-file-hosting-v1.md \
    crates/oo-control/Cargo.toml \
    crates/oo-control/src/lib.rs
  do
    fixed_blob=$(git -C "$SRC" rev-parse "${FIXED}:${path}")
    head_blob=$(git -C "$SRC" rev-parse "HEAD:${path}")
    work_hash=$(shasum -a 256 "$SRC/$path" | awk '{print $1}')
    echo "fixed=$fixed_blob head=$head_blob work=$work_hash $path"
  done
} > "$EVIDENCE/archive-base.txt"

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}'
  echo "host_toolchain_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver|rustup)$/ {print}' || true
} > "$EVIDENCE/builders-before.txt"

running_rust=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
own=$(docker ps -q --filter label=oo.owner=a186 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
printf 'running_rust_builders=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 3 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive HEAD | tar -x -C "$SNAP"
cp "$SRC/crates/oo-control/src/file_hosting.rs" "$SNAP/crates/oo-control/src/file_hosting.rs"
cp "$SRC/crates/oo-control/tests/file_hosting.rs" "$SNAP/crates/oo-control/tests/file_hosting.rs"
cp "$SRC/contracts/control/local-file-hosting-v1.md" "$SNAP/contracts/control/local-file-hosting-v1.md"
(
  cd "$SNAP"
  shasum -a 256 \
    crates/oo-control/src/file_hosting.rs \
    crates/oo-control/tests/file_hosting.rs \
    contracts/control/local-file-hosting-v1.md \
    crates/oo-control/Cargo.toml \
    crates/oo-control/src/lib.rs
) > "$EVIDENCE/overlay-before.sha256"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$RUST_IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
grep -q 'os=linux' "$EVIDENCE/image.txt"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "private network missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect "$NET" --format 'name={{.Name}} driver={{.Driver}} internal={{.Internal}}' > "$EVIDENCE/network.txt"
grep -q 'internal=false' "$EVIDENCE/network.txt"

if ! docker volume inspect "$CARGO_VOL" >/dev/null 2>&1; then
  echo "cargo volume missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker volume inspect "$TARGET_VOL" >/dev/null 2>&1; then
  echo "target volume missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker volume inspect "$CARGO_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-cargo.txt"
docker volume inspect "$TARGET_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-target.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named post-edit container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
fixed=${FIXED}
workspace_head=$(git -C "$SRC" rev-parse HEAD)
composition=git archive of HEAD plus working-tree overlay of file_hosting.rs, tests/file_hosting.rs, and local-file-hosting-v1.md
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
metrics=OO_FH_METRICS_DIR=/evidence/metrics
phases=fmt fmt-check fetch lock-unchanged test clippy
package=oo-control
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
  -e OO_FH_METRICS_DIR=/evidence/metrics \
  --mount "type=bind,source=${SNAP},target=/src" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a186 \
  --label oo.session="$SESSION" \
  --label oo.task=local-hosting-bounds-post-edit \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"

(
  cd "$SNAP"
  shasum -a 256 \
    crates/oo-control/src/file_hosting.rs \
    crates/oo-control/tests/file_hosting.rs \
    contracts/control/local-file-hosting-v1.md \
    crates/oo-control/Cargo.toml \
    crates/oo-control/src/lib.rs
) > "$EVIDENCE/overlay-after.sha256"

: > "$EVIDENCE/fmt-sync.txt"
for path in \
  crates/oo-control/src/file_hosting.rs \
  crates/oo-control/tests/file_hosting.rs
do
  if ! cmp -s "$SNAP/$path" "$SRC/$path"; then
    cp "$SNAP/$path" "$SRC/$path"
    echo "copied_back $path" >> "$EVIDENCE/fmt-sync.txt"
  else
    echo "unchanged $path" >> "$EVIDENCE/fmt-sync.txt"
  fi
done

exit "$run_exit"
