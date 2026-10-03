#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder for the unmodified author tests.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a130/SP03/reader-primitives-review/linux/author
SMOKE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a130/SP03/reader-primitives-review/linux/smoke
ROOT=/tmp/oo-a130-eef831aa-dba4-4af2-a78b-541bc9e0a130
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=eef831aa-dba4-4af2-a78b-541bc9e0a130-rust-author
NET=eef831aa-dba4-4af2-a78b-541bc9e0a130-net
CARGO_VOL=eef831aa-dba4-4af2-a78b-541bc9e0a130-cargo
TARGET_VOL=eef831aa-dba4-4af2-a78b-541bc9e0a130-target
PGLOG=eef831aa-dba4-4af2-a78b-541bc9e0a130-pglog
SEC=/tmp/oo-a130-eef831aa-dba4-4af2-a78b-541bc9e0a130/secrets
COMMIT=ccd93e1adc34b24ab998d90972549f8cffb59f0d

mkdir -p "$EVIDENCE"
test "$(tr -d '[:space:]' < "$SMOKE/lib.exit")" = "0"
test "$(tr -d '[:space:]' < "$SMOKE/clippy.exit")" = "0"
cp "$EVIDENCE/inside.sh" "$ROOT/inside-author.sh"
chmod 755 "$ROOT/inside-author.sh" "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
rm -f "$EVIDENCE"/*.exit "$EVIDENCE"/console.log
cp "$SMOKE/snapshot-files.sha256" "$EVIDENCE/snapshot-files.sha256"
test ! -f "$ROOT/smoke-snapshot/crates/oo-connectors/tests/a130_review.rs"

running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $2 ~ /0f24958fa693/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a130 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${COMMIT}
snapshot=${ROOT}/smoke-snapshot
snapshot_method=git archive of ${COMMIT} only
overlay=none
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
tests=catalog_rate_live snapshot_live pg_portal_bounds pg_portal_live
test_threads=1
pull=never
privileged=no
cap_drop=ALL
cap_add=none
memory=8g
cpus=4
pids_limit=4096
host_port=none
socket_mount=no
home_mount=no
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
  -e OO_PG_SNAPSHOT_FIXTURE=/secrets/pointer.json \
  -e OO_PG_CATALOG_RATE_FIXTURE=/secrets/pointer.json \
  -e OO_PG_SNAPSHOT_OUTCOME=/evidence/snapshot-outcome.txt \
  -e OO_A127_FIXTURE=/secrets/portal-pointer.txt \
  -e OO_A127_LOG_REPORT=/evidence/portal-log-class.txt \
  --mount "type=bind,source=${ROOT}/smoke-snapshot,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/inside-author.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=volume,source=${PGLOG},target=/pglog" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a130 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a130 \
  --label oo.task=reader-primitives-review-author \
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
  echo "MemorySwap=$(docker inspect -f '{{.HostConfig.MemorySwap}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{.HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{.HostConfig.CapAdd}}' "$NAME")"
  echo "SecurityOpt=$(docker inspect -f '{{.HostConfig.SecurityOpt}}' "$NAME")"
  docker inspect -f '{{range .Mounts}}mount dest={{.Destination}} type={{.Type}} rw={{.RW}}{{println}}{{end}}' "$NAME"
} > "$EVIDENCE/hostconfig.txt"
printf '%s\n' 0 > "$EVIDENCE/inspect.exit"
exit "$run_exit"
