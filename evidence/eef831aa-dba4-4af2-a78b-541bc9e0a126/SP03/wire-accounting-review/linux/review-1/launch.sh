#!/bin/bash
# Author tests and the independent probe on a git archive of ff783ac9.
# The probe is the only file added. Product bytes stay the fixed commit.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a126/SP03/wire-accounting-review/linux/review-1
PROBE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a126/SP03/wire-accounting-review/probe/pg_wire_accounting_review.rs
ROOT=/tmp/oo-a126-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a126-msrv-review
NET=oo-a126-eef831aa-net
CARGO_VOL=oo-a126-eef831aa-cargo
TARGET_VOL=oo-a126-eef831aa-target
COMMIT=ff783ac933b86e0ff068181afe0258764c5d2e5d
TREE=3a5554c7e8ff09555924675b60cb287155398caf
PARENT=1150151bf82a1c72997b1c3aaacb306d47e16cd0

mkdir -p "$EVIDENCE" "$ROOT"
cp "$EVIDENCE/inside.sh" "$ROOT/inside-review.sh"
chmod 755 "$ROOT/inside-review.sh" "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
rm -f "$EVIDENCE"/*.exit "$EVIDENCE"/console.log

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt"
ps -axo pid=,ppid=,etime=,comm= > "$EVIDENCE/processes-comm-only.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a126 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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

git -C "$SRC" rev-parse "$COMMIT" > "$EVIDENCE/source-head.txt"
git -C "$SRC" rev-parse "${COMMIT}^{tree}" > "$EVIDENCE/source-tree.txt"
git -C "$SRC" rev-parse "${COMMIT}^" > "$EVIDENCE/source-parent.txt"
test "$(cat "$EVIDENCE/source-head.txt")" = "$COMMIT"
test "$(cat "$EVIDENCE/source-tree.txt")" = "$TREE"
test "$(cat "$EVIDENCE/source-parent.txt")" = "$PARENT"

# Reuse the smoke archive directory so the fixed bytes stay put, then overlay
# only the probe. Rebuild the archive if the smoke snapshot is missing.
if [ ! -d "$ROOT/smoke-snapshot/crates/oo-connectors" ]; then
  rm -rf "$ROOT/smoke-snapshot"
  mkdir -p "$ROOT/smoke-snapshot"
  git -C "$SRC" archive --format=tar "$COMMIT" \
    Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo \
    contracts/source/pgsql/empty-catalog.json \
    | tar -C "$ROOT/smoke-snapshot" -xf -
fi
cp "$PROBE" "$ROOT/smoke-snapshot/crates/oo-connectors/tests/pg_wire_accounting_review.rs"
(
  cd "$ROOT/smoke-snapshot"
  shasum -a 256 \
    crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs \
    crates/oo-connectors/vendor/sqlx-postgres/src/connection/stream.rs \
    crates/oo-connectors/vendor/sqlx-postgres/src/connection/mod.rs \
    crates/oo-connectors/vendor/PATCH.md \
    crates/oo-connectors/tests/pg_wire_bounds.rs \
    crates/oo-connectors/tests/pg_wire_accounting.rs
) > "$EVIDENCE/snapshot-files.sha256"
(
  cd "$ROOT/smoke-snapshot"
  shasum -a 256 crates/oo-connectors/tests/pg_wire_accounting_review.rs
) > "$EVIDENCE/probe.sha256"
# Product manifest must still be the fixed commit, byte for byte.
cmp -s "$EVIDENCE/snapshot-files.sha256" \
  "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a126/SP03/wire-accounting-review/linux/smoke/snapshot-files.sha256"
printf '%s\n' 0 > "$EVIDENCE/snapshot-prep.exit"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"

if docker network inspect "$NET" >/dev/null 2>&1; then
  echo "network_exists=$NET" > "$EVIDENCE/network.txt"
else
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a126 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a126 \
    "$NET" > "$EVIDENCE/network.txt"
fi
docker volume create --label oo.owner=a126 --label oo.project=open-ontology "$CARGO_VOL" > "$EVIDENCE/cargo-volume.txt"
docker volume create --label oo.owner=a126 --label oo.project=open-ontology "$TARGET_VOL" > "$EVIDENCE/target-volume.txt"

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  printf 'prior_status=%s\n' "$prior" > "$EVIDENCE/prior-container.txt"
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >> "$EVIDENCE/prior-container.txt"
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${COMMIT}
source_parent=${PARENT}
source_tree=${TREE}
snapshot_method=git archive of ${COMMIT} plus probe overlay only
workspace_not_used=yes
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
network_mode=bridge_not_host
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/smoke-snapshot
privileged=no
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
cap_drop=ALL
cap_add=none
init=yes
no_new_privileges=yes
memory=8g
memory_swap=8g
cpus=4
pids_limit=4096
user=0:0
src_mount=readonly
fetch=cargo fetch --manifest-path Cargo.toml --locked --offline
lib=cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
bounds=cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_bounds -- --test-threads=1 --nocapture
accounting=cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_accounting -- --test-threads=1 --nocapture
review=cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_accounting_review -- --test-threads=1 --nocapture
clippy_review=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_accounting_review -- -D warnings
airgap_claim=no
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
  -e RUST_BACKTRACE=1 \
  -e HOME=/tmp \
  --mount "type=bind,source=${ROOT}/smoke-snapshot,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/inside-review.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a126 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a126 \
  --label oo.task=wire-accounting-review-1 \
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
