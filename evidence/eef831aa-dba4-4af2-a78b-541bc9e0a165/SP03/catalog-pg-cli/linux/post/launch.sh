#!/bin/bash
# One post-edit rust builder. Reuses the a165 cargo and target volumes. No host toolchain.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
BASE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a165/SP03/catalog-pg-cli
EVIDENCE=${BASE}/linux/post
ROOT=/tmp/oo-a165-eef831aa-dba4-4af2-a78b-541bc9e0a165
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a165-post
NET=oo-a165-eef831aa-net
CARGO_VOL=oo-a165-eef831aa-cargo
TARGET_VOL=oo-a165-eef831aa-target
SNAP=${ROOT}/post-src
SEC=${ROOT}/secrets
CA=${ROOT}/ca

mkdir -p "$EVIDENCE"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite post console.log" >&2
  exit 2
fi
if [ ! -f "$BASE/pg/ready.exit" ] || [ "$(cat "$BASE/pg/ready.exit")" != "0" ]; then
  echo "postgres is not ready" >&2
  exit 2
fi
for secret in admin_password reader_password writer_password; do
  if [ ! -f "$SEC/$secret" ]; then
    echo "missing password file" >&2
    exit 2
  fi
done
if [ ! -f "$CA/ca.crt" ]; then
  echo "missing private ca" >&2
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
  echo "a165 network is absent" >&2
  exit 2
fi
docker network inspect --format '{{.Id}} {{.Driver}}' "$NET" > "$EVIDENCE/network.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a165 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a165 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s global_rust_builders=%s own_postgres=%s\n' "$running_rust" "$own" "$host_cargo" "$global_builders" "$own_pg" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ] || [ "$global_builders" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if [ "$own_pg" -ne 1 ]; then
  echo "expected exactly one a165 postgres" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive HEAD | tar -x -C "$SNAP"
if [ -e "$SNAP/crates/oo-pipeline/src/catalog_cli.rs" ]; then
  echo "HEAD archive already contains the catalog command" >&2
  exit 2
fi
if [ ! -f "$SNAP/crates/oo-pipeline/src/import_cli.rs" ]; then
  echo "archive is missing the import command" >&2
  exit 2
fi
cp "$SRC/crates/oo-pipeline/src/catalog_cli.rs" "$SNAP/crates/oo-pipeline/src/catalog_cli.rs"
cp "$SRC/crates/oo-pipeline/src/import_cli.rs" "$SNAP/crates/oo-pipeline/src/import_cli.rs"
cp "$SRC/crates/oo-pipeline/src/lib.rs" "$SNAP/crates/oo-pipeline/src/lib.rs"
cp "$SRC/crates/oo-pipeline/src/bin/oo-pipeline.rs" "$SNAP/crates/oo-pipeline/src/bin/oo-pipeline.rs"
cp "$SRC/crates/oo-pipeline/tests/catalog_pg_cli_live.rs" "$SNAP/crates/oo-pipeline/tests/catalog_pg_cli_live.rs"
mkdir -p "$SNAP/contracts/pipeline"
cp "$SRC/contracts/pipeline/catalog-pg-cli-v1.md" "$SNAP/contracts/pipeline/catalog-pg-cli-v1.md"
(
  cd "$SRC"
  shasum -a 256 \
    crates/oo-pipeline/src/catalog_cli.rs \
    crates/oo-pipeline/src/import_cli.rs \
    crates/oo-pipeline/src/lib.rs \
    crates/oo-pipeline/src/bin/oo-pipeline.rs \
    crates/oo-pipeline/tests/catalog_pg_cli_live.rs \
    contracts/pipeline/catalog-pg-cli-v1.md
) > "$EVIDENCE/overlay.sha256"
git -C "$SRC" diff --stat -- \
  crates/oo-pipeline/src/catalog_cli.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  crates/oo-pipeline/tests/catalog_pg_cli_live.rs \
  contracts/pipeline/catalog-pg-cli-v1.md > "$EVIDENCE/overlay-stat.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named post container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cat > "$EVIDENCE/invocation.txt" <<EOF
workspace_head=${head_now}
composition=git archive HEAD plus owned catalog overlay
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
postgres_host=oo-a165-pg
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
password_env=no
ca_mount=/private-ca/ca.crt
phases=toolchain clippy-if-missing build-bins pipeline-lib import-cli catalog-cli pipeline-clippy captured-cli review-cli catalog-live
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
  -e OO_CATALOG_PG_HOST=oo-a165-pg \
  -e OO_CATALOG_PG_PORT=5432 \
  -e OO_CATALOG_PG_ADMIN_USER=oo_a165_admin \
  -e OO_CATALOG_PG_ADMIN_PASSWORD_FILE=/run/secrets/admin_password \
  -e OO_CATALOG_PG_READER_PASSWORD_FILE=/run/secrets/reader_password \
  -e OO_CATALOG_PG_WRITER_PASSWORD_FILE=/run/secrets/writer_password \
  -e OO_CATALOG_PG_CA_FILE=/private-ca/ca.crt \
  -e OO_CATALOG_PG_EVIDENCE=/evidence/out \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${CA}/ca.crt,target=/private-ca/ca.crt,readonly" \
  --mount "type=bind,source=${SEC}/admin_password,target=/run/secrets/admin_password,readonly" \
  --mount "type=bind,source=${SEC}/reader_password,target=/run/secrets/reader_password,readonly" \
  --mount "type=bind,source=${SEC}/writer_password,target=/run/secrets/writer_password,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a165 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a165 \
  --label oo.task=catalog-pg-cli-post \
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
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
