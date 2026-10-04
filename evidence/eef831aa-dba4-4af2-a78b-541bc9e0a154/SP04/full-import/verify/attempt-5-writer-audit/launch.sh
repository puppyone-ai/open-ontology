#!/bin/bash
# Immutable fixed-base archive plus the owned full-import overlay.
# One Rust 1.94 builder. PostgreSQL is started by pg/up.sh before this container.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a154/SP04/full-import/verify
ROOT=/tmp/oo-a154-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a154-full-import-verify
PG=oo-a154-pg
NET=oo-a154-eef831aa-net
CARGO_VOL=oo-a154-eef831aa-cargo
TARGET_VOL=oo-a154-eef831aa-target
FIXED=bedd6a2ced65edf6a44ac85ecd40551e1b79a45c
SNAP=${ROOT}/verify-snapshot
SEC=${ROOT}/secrets

mkdir -p "$EVIDENCE/live" "$EVIDENCE/installer"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh" "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a154/SP04/full-import/pg/up.sh" "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a154/SP04/full-import/pg/pg-wrap.sh"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"

if docker ps --filter name=^/oo-a154-pg$ --filter status=running --format '{{.Names}}' | grep -q .; then
  echo "reusing running postgres oo-a154-pg"
else
  bash "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a154/SP04/full-import/pg/up.sh"
fi

if [ ! -s "$SEC/admin_password" ] || [ ! -s "$SEC/reader_password" ] || [ ! -s "$SEC/runtime_password" ] || [ ! -s "$SEC/ca.crt" ]; then
  echo "password or CA file is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$SEC/server.key" ] || [ -f "$SEC/ca.key" ]; then
  echo "private key must not sit in the builder secret mount" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a154 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a running rust builder" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "named verify container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" > "$EVIDENCE/prior-container.txt"
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo contracts \
  | tar -x -C "$SNAP"
overlay='
crates/oo-pipeline/src/lib.rs
crates/oo-pipeline/src/full_import.rs
crates/oo-pipeline/src/copy_sink.rs
crates/oo-pipeline/src/target_installer.rs
crates/oo-pipeline/migrations/0005_full_import_expand.sql
crates/oo-pipeline/migrations/README.md
crates/oo-pipeline/tests/full_import_live.rs
crates/oo-pipeline/tests/target_installer_live.rs
contracts/pipeline/full-import-v1.md
contracts/pipeline/full-import-v1.json
'
for path in $overlay; do
  mkdir -p "$SNAP/$(dirname "$path")"
  cp "$SRC/$path" "$SNAP/$path"
done
test -f "$SNAP/crates/oo-pipeline/src/full_import.rs"
test -f "$SNAP/crates/oo-pipeline/migrations/0005_full_import_expand.sql"
shasum -a 256 "$SNAP/crates/oo-pipeline/migrations/0005_full_import_expand.sql" > "$EVIDENCE/migration-0005.sha256"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/source-tree.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/source-commit.txt"

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${FIXED}
tree=$(cat "$EVIDENCE/source-tree.txt")
snapshot_method=git archive of product paths from ${FIXED} plus owned overlay
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
password_transport=file
private_key_mount=no
phases=lib,installer,full-import,clippy
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
  -e OO_TARGET_INSTALLER_HOST=oo-a154-pg \
  -e OO_TARGET_INSTALLER_PORT=5432 \
  -e OO_TARGET_INSTALLER_USER=oo_fixture_admin \
  -e OO_TARGET_INSTALLER_PASSWORD_FILE=/run/secrets/admin_password \
  -e OO_TARGET_INSTALLER_EVIDENCE=/evidence/installer \
  -e OO_FULL_IMPORT_HOST=oo-a154-pg \
  -e OO_FULL_IMPORT_PORT=5432 \
  -e OO_FULL_IMPORT_ADMIN_USER=oo_fixture_admin \
  -e OO_FULL_IMPORT_ADMIN_PASSWORD_FILE=/run/secrets/admin_password \
  -e OO_FULL_IMPORT_READER_PASSWORD_FILE=/run/secrets/reader_password \
  -e OO_FULL_IMPORT_RUNTIME_PASSWORD_FILE=/run/secrets/runtime_password \
  -e OO_FULL_IMPORT_CA_FILE=/run/secrets/ca.crt \
  -e OO_FULL_IMPORT_EVIDENCE=/evidence/live \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/run/secrets,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a154 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a154 \
  --label oo.task=full-import-verify \
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
  echo "Status=$(docker inspect -f '{{.State.Status}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
exit "$run_exit"
