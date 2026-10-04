#!/bin/bash
# Second archive of cc764cbc plus the review oracle only. One rust builder.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a159/SP04/full-import-review
EVIDENCE=${BASE}/linux/probe
SMOKE=${BASE}/linux/smoke
ROOT=/tmp/oo-a159-eef831aa-dba4-4af2-a78b-541bc9e0a159
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a159-probe
SMOKE_NAME=oo-a159-smoke
NET=oo-a159-eef831aa-net
CARGO_VOL=oo-a159-eef831aa-cargo
TARGET_VOL=oo-a159-eef831aa-target
FIXED=cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4
EXPECT_TREE=da85327b402378cfdc3b826300205569c43b3642
SNAP=${ROOT}/probe-src
OVERLAY=${BASE}/overlay/full_import_a159_review.rs

mkdir -p "$EVIDENCE/live"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite probe console.log" >&2
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "a159 network is absent" >&2
  exit 2
fi
if ! docker ps --format '{{.Names}}' | grep -qx 'oo-a159-pg'; then
  echo "private postgres is not running" >&2
  exit 2
fi
if docker container inspect "$SMOKE_NAME" >/dev/null 2>&1; then
  smoke_status=$(docker inspect -f '{{.State.Status}}' "$SMOKE_NAME")
  if [ "$smoke_status" != "exited" ]; then
    echo "smoke builder is still ${smoke_status}" >&2
    exit 3
  fi
  docker rm "$SMOKE_NAME" >/dev/null
fi
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a159 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
test "$actual_tree" = "$EXPECT_TREE"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
if [ -e "$SNAP/crates/oo-pipeline/tests/full_import_a159_review.rs" ]; then
  echo "fixed archive already contains the review overlay" >&2
  exit 2
fi
cp "$OVERLAY" "$SNAP/crates/oo-pipeline/tests/full_import_a159_review.rs"
file_sha() { shasum -a 256 "$1" | awk '{print $1}'; }
cmp_sha() {
  got=$(file_sha "$2")
  exp=$(tr -d '[:space:]' < "$1")
  test "$got" = "$exp"
}
cmp_sha "$SMOKE/expect-cargo.sha256" "$SNAP/crates/oo-pipeline/Cargo.toml"
cmp_sha "$SMOKE/expect-lib.sha256" "$SNAP/crates/oo-pipeline/src/lib.rs"
cmp_sha "$SMOKE/expect-full-import.sha256" "$SNAP/crates/oo-pipeline/src/full_import.rs"
cmp_sha "$SMOKE/expect-sink.sha256" "$SNAP/crates/oo-pipeline/src/copy_sink.rs"
cmp_sha "$SMOKE/expect-encoder.sha256" "$SNAP/crates/oo-pipeline/src/copy_encoder.rs"
cmp_sha "$SMOKE/expect-mapping.sha256" "$SNAP/crates/oo-pipeline/src/physical_mapping.rs"
cmp_sha "$SMOKE/expect-installer.sha256" "$SNAP/crates/oo-pipeline/src/target_installer.rs"
cmp_sha "$SMOKE/expect-0005.sha256" "$SNAP/crates/oo-pipeline/migrations/0005_full_import_expand.sql"
cmp_sha "$SMOKE/expect-contract-json.sha256" "$SNAP/contracts/pipeline/full-import-v1.json"
file_sha "$OVERLAY" > "$EVIDENCE/overlay.sha256"
file_sha "$SNAP/crates/oo-pipeline/tests/full_import_a159_review.rs" > "$EVIDENCE/overlay-copied.sha256"
cmp "$EVIDENCE/overlay.sha256" "$EVIDENCE/overlay-copied.sha256"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named probe container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cat > "$EVIDENCE/invocation.txt" <<EOF
fixed=${FIXED}
tree=${EXPECT_TREE}
composition=git archive plus tests/full_import_a159_review.rs
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
postgres_alias=oo-a159-pg
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
server_key_mounted=no
tls=verify-full
phases=composition-oracle
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
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e OO_A159_HOST=oo-a159-pg \
  -e OO_A159_PORT=5432 \
  -e OO_A159_ADMIN_USER=oo_fixture_admin \
  -e OO_A159_ADMIN_PASSWORD_FILE=/run/secrets/admin_password \
  -e OO_A159_READER_PASSWORD_FILE=/run/secrets/reader_password \
  -e OO_A159_RUNTIME_PASSWORD_FILE=/run/secrets/runtime_password \
  -e OO_A159_CA_FILE=/private-ca/ca.crt \
  -e OO_A159_EVIDENCE=/evidence/live \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${ROOT}/ca,target=/private-ca,readonly" \
  --mount "type=bind,source=${ROOT}/secrets,target=/run/secrets,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a159 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a159 \
  --label oo.task=full-import-review-probe \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
exit "$run_exit"
