#!/bin/bash
# Second review builder. Attempt 1 console.log stays untouched.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a194/SP05/local-hosting-download-review
EVIDENCE=${REVIEW}/oracle-attempt-4
OVERLAY=${REVIEW}/oracle/review_download.rs
PRE=${REVIEW}/pre-edit
ROOT=/tmp/oo-a194-eef831aa-dba4-4af2-a78b-541bc9e0a194
FIXED=e3363ffecc21af4dc9fcf75509978da34b0860b1
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a194-review4
NET=oo-a194-eef831aa-net
CARGO_VOL=oo-a194-eef831aa-cargo
TARGET_VOL=oo-a194-eef831aa-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a194
SNAP=${ROOT}/archive
INSIDE=${REVIEW}/oracle/inside.sh

mkdir -p "$EVIDENCE/phases" "$EVIDENCE/notes" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite attempt 4 console.log" >&2
  exit 2
fi
if [ ! -f "${REVIEW}/oracle/console.log" ] || [ ! -f "${REVIEW}/oracle-attempt-2/console.log" ] || [ ! -f "${REVIEW}/oracle-attempt-3/console.log" ]; then
  echo "an earlier attempt console.log is missing" >&2
  exit 2
fi
pre_run=$(cat "$PRE/docker-run.exit")
pre_worst=$(cat "$PRE/phases/worst.exit")
if [ "$pre_run" != "0" ] || [ "$pre_worst" != "0" ]; then
  echo "refusing overlay: pre-overlay smoke is not green" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
chmod 755 "$INSIDE" "$EVIDENCE/launch.sh"

cleanup() {
  docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt" 2>/dev/null || true
  docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} readonly_root={{.HostConfig.ReadonlyRootfs}} memory={{.HostConfig.Memory}} memoryswap={{.HostConfig.MemorySwap}}' > "$EVIDENCE/hostconfig.txt" 2>/dev/null || true
  docker inspect "$NAME" --format '{{json .HostConfig.Binds}}{{json .HostConfig.Mounts}}' > "$EVIDENCE/mounts.txt" 2>/dev/null || true
  docker rm -f "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

{
  echo "attempt=4"
  echo "fixed=$FIXED"
  echo "workspace_head=$(git -C "$SRC" rev-parse HEAD)"
  echo "fixed_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")"
  echo "pre_overlay_docker_run=$pre_run"
  echo "attempt1_docker_run=$(cat "${REVIEW}/oracle/docker-run.exit")"
  echo "attempt2_docker_run=$(cat "${REVIEW}/oracle-attempt-2/docker-run.exit")"
  echo "attempt3_docker_run=$(cat "${REVIEW}/oracle-attempt-3/docker-run.exit")"
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
own=$(docker ps -q --filter label=oo.owner=a194 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
printf 'running_rust_builders=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 6 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/archive-tree.txt"
cp "$OVERLAY" "$SNAP/crates/oo-control/tests/review_download.rs"
cp "$OVERLAY" "$EVIDENCE/review_download.input.rs"

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
    blob=$(git -C "$SRC" rev-parse "${FIXED}:${path}")
    snap_blob=$(git -C "$SNAP" hash-object "$path")
    sha=$(shasum -a 256 "$SNAP/$path" | awk '{print $1}')
    match=0
    if [ "$blob" = "$snap_blob" ]; then
      match=1
    else
      ok=0
    fi
    echo "path=$path blob=$blob snap=$snap_blob sha256=$sha match=$match"
  done
  overlay_sha=$(shasum -a 256 "$SNAP/crates/oo-control/tests/review_download.rs" | awk '{print $1}')
  echo "overlay=crates/oo-control/tests/review_download.rs sha256=$overlay_sha"
  echo "archive_hashes_match=$ok"
  if [ "$ok" != 1 ]; then
    echo "archive product bytes differ from the fixed commit" >&2
    printf '%s\n' 5 > "$EVIDENCE/docker-run.exit"
    exit 5
  fi
} > "$EVIDENCE/archive-match.txt"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$RUST_IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
grep -q 'os=linux' "$EVIDENCE/image.txt"
grep -q 'sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5' "$EVIDENCE/image.txt"

docker network inspect "$NET" --format 'name={{.Name}} driver={{.Driver}} internal={{.Internal}}' > "$EVIDENCE/network.txt"
grep -q 'internal=false' "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-cargo.txt"
docker volume inspect "$TARGET_VOL" --format 'name={{.Name}}' > "$EVIDENCE/volume-target.txt"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named review container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
attempt=4
fixed=${FIXED}
tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
composition=git archive of ${FIXED} plus oracle/review_download.rs
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
phases=rustfmt-overlay fmt-check test clippy
package=oo-control
test=review_download
file_hosting_suite=not_rerun
hash_oracle=sha2 and shasum and sha256sum; no python hashlib
notes=OO_A194_NOTES=/evidence/notes
attempt1_preserved=${REVIEW}/oracle/console.log
attempt2_preserved=${REVIEW}/oracle-attempt-2/console.log
attempt3_preserved=${REVIEW}/oracle-attempt-3/console.log
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
  -e OO_A194_NOTES=/evidence/notes \
  --mount "type=bind,source=${SNAP},target=/src" \
  --mount "type=bind,source=${INSIDE},target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a194 \
  --label oo.session="$SESSION" \
  --label oo.task=local-hosting-download-review-oracle-attempt-4 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"

if [ -f "$SNAP/crates/oo-control/tests/review_download.rs" ] && ! cmp -s "$OVERLAY" "$SNAP/crates/oo-control/tests/review_download.rs"; then
  cp "$OVERLAY" "$EVIDENCE/review_download.pre-fmt.rs"
  cp "$SNAP/crates/oo-control/tests/review_download.rs" "$OVERLAY"
  cp "$SNAP/crates/oo-control/tests/review_download.rs" "$EVIDENCE/review_download.executed.rs"
  echo "fmt_synced=1" > "$EVIDENCE/fmt-sync.txt"
else
  cp "$SNAP/crates/oo-control/tests/review_download.rs" "$EVIDENCE/review_download.executed.rs"
  echo "fmt_synced=0" > "$EVIDENCE/fmt-sync.txt"
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
    blob=$(git -C "$SRC" rev-parse "${FIXED}:${path}")
    snap_blob=$(git -C "$SNAP" hash-object "$path")
    match=0
    if [ "$blob" = "$snap_blob" ]; then
      match=1
    else
      ok=0
    fi
    echo "path=$path blob=$blob snap=$snap_blob match=$match"
  done
  echo "archive_unchanged_after_overlay=$ok"
  if [ "$ok" != 1 ]; then
    echo "overlay run mutated a product file" >&2
    exit 5
  fi
} > "$EVIDENCE/archive-after.txt"

exit "$run_exit"
