#!/bin/bash
# Fixed archive plus the independent review test. One rust builder. Private PostgreSQL already running.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a152/SP04/target-installer-review
EVIDENCE=${BASE}/linux/probe
ROOT=/tmp/oo-a152-eef831aa-dba4-4af2-a78b-541bc9e0a152
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a152-probe
PG=oo-a152-pg
NET=oo-a152-eef831aa-net
CARGO_VOL=oo-a152-eef831aa-cargo
TARGET_VOL=oo-a152-eef831aa-target
FIXED=4ed22b8c1189e72b06b20a45335860ac9a3ea44e
EXPECT_TREE=fafb0fe6752b8240eb2b29025fc38bccdfb7144d
SNAP=${ROOT}/probe-src

mkdir -p "$EVIDENCE/live"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite probe console.log" >&2
  exit 2
fi
head_now=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$head_now" > "$EVIDENCE/workspace-head.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "a152 network is absent" >&2
  exit 2
fi
docker network inspect --format '{{.Id}} {{.Driver}}' "$NET" > "$EVIDENCE/network.txt"
if ! docker container inspect "$PG" >/dev/null 2>&1; then
  echo "private postgres is absent" >&2
  exit 2
fi
if [ ! -s "$ROOT/pg-secrets/admin_password" ]; then
  echo "password file is absent" >&2
  exit 2
fi
docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a152 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
running_pg=$(docker ps -q --filter label=oo.owner=a152 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s own_postgres_running=%s\n' "$running_rust" "$own" "$host_cargo" "$running_pg" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if [ "$running_pg" -ne 1 ]; then
  echo "expected exactly one a152 postgres" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
test "$actual_tree" = "$EXPECT_TREE"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
cp "${BASE}/overlay/target_installer_a152_review.rs" "$SNAP/crates/oo-pipeline/tests/target_installer_a152_review.rs"
file_sha() { shasum -a 256 "$1" | awk '{print $1}'; }
file_sha "$SNAP/crates/oo-pipeline/src/target_installer.rs" > "$EVIDENCE/expect-installer.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/lib.rs" > "$EVIDENCE/expect-lib.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/physical_mapping.rs" > "$EVIDENCE/expect-mapping.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/copy_encoder.rs" > "$EVIDENCE/expect-encoder.sha256"
file_sha "$SNAP/crates/oo-pipeline/migrations/0000_bootstrap_roles.sql" > "$EVIDENCE/expect-0000.sha256"
file_sha "$SNAP/crates/oo-pipeline/migrations/0001_target_fence_expand.sql" > "$EVIDENCE/expect-0001.sha256"
file_sha "$SNAP/crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql" > "$EVIDENCE/expect-0002.sha256"
file_sha "$SNAP/crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql" > "$EVIDENCE/expect-0003.sha256"
file_sha "$SNAP/crates/oo-pipeline/migrations/0004_physical_mapping.sql" > "$EVIDENCE/expect-0004.sha256"
file_sha "$SNAP/crates/oo-pipeline/tests/target_installer_a152_review.rs" > "$EVIDENCE/expect-overlay.sha256"
file_sha "$SNAP/Cargo.lock" > "$EVIDENCE/expect-lock.sha256"
test "$(file_sha "${BASE}/overlay/target_installer_a152_review.rs")" = "$(awk '{print $1}' "$EVIDENCE/expect-overlay.sha256")"
{
  echo "installer $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/src/target_installer.rs")"
  echo "lib $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/src/lib.rs")"
  echo "sql0000 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0000_bootstrap_roles.sql")"
  echo "sql0001 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0001_target_fence_expand.sql")"
  echo "sql0002 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql")"
  echo "sql0003 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0003_target_segment_preflight_expand.sql")"
  echo "sql0004 $(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/migrations/0004_physical_mapping.sql")"
  echo "lock $(git -C "$SRC" rev-parse "${FIXED}:Cargo.lock")"
} > "$EVIDENCE/fixed-blobs.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named probe container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cat > "$EVIDENCE/invocation.txt" <<EOF
workspace_head=${head_now}
fixed=${FIXED}
tree=${EXPECT_TREE}
composition=git archive plus overlay
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
postgres=${PG}
network=${NET}
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
host_ca_mounted=no
password_transport=file
ssl_mode=disable
phases=review
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
  -e OO_A152_HOST=oo-a152-pg \
  -e OO_A152_PORT=5432 \
  -e OO_A152_USER=oo_fixture_admin \
  -e OO_A152_PASSWORD_FILE=/run/secrets/admin_password \
  -e OO_A152_EVIDENCE=/evidence/live \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${ROOT}/pg-secrets,target=/run/secrets,readonly" \
  --mount "type=bind,source=${ROOT}/ca,target=/private-ca,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a152 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a152 \
  --label oo.task=target-installer-review-probe \
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
  echo "CapDrop=$(docker inspect -f '{{.HostConfig.CapDrop}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
exit "$run_exit"
