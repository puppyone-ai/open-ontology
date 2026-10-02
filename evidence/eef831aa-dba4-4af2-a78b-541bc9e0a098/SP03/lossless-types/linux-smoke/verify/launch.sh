#!/bin/bash
# Frozen snapshot of HEAD plus the type-decoder overlay. Does not use the dirty worktree lock.
set -euo pipefail
RUN_ID="${1:?run id}"
ROOT="/tmp/oo-a098-${RUN_ID}"
SNAP="$ROOT/snapshot"
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a098
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE="$REPO/evidence/${SESSION}/SP03/lossless-types"
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME="oo-a098-msrv-${RUN_ID}"
NET="oo-a098-pg-${RUN_ID}-net"
CARGO_VOL="oo-a098-cargo-${RUN_ID}"
TARGET_VOL="oo-a098-target-${RUN_ID}"

if ! docker image inspect "$RUST_IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi
mkdir -p "$SNAP" "$EVIDENCE/linux-smoke/verify/phases"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$REPO" archive --format=tar HEAD | tar -x -C "$SNAP"
cp "$REPO/crates/oo-connectors/src/lib.rs" "$SNAP/crates/oo-connectors/src/lib.rs"
cp "$REPO/crates/oo-connectors/src/session.rs" "$SNAP/crates/oo-connectors/src/session.rs"
cp "$REPO/crates/oo-connectors/src/sql.rs" "$SNAP/crates/oo-connectors/src/sql.rs"
rm -rf "$SNAP/crates/oo-connectors/src/types"
cp -R "$REPO/crates/oo-connectors/src/types" "$SNAP/crates/oo-connectors/src/types"
cp "$REPO/crates/oo-connectors/tests/lossless_live.rs" "$SNAP/crates/oo-connectors/tests/lossless_live.rs"
cp "$REPO/crates/oo-connectors/tests/pg_wire_oracle.rs" "$SNAP/crates/oo-connectors/tests/pg_wire_oracle.rs"
cp "$REPO/crates/oo-connectors/Cargo.toml" "$SNAP/crates/oo-connectors/Cargo.toml"
# Candidate resolution only: HEAD lock plus the Arrow edges attempt 3 fetched.
# This run must prove it with --locked. The committed lock is the file this
# container actually fetches, after that file is diffed against HEAD.
cp "$REPO/evidence/${SESSION}/SP03/lossless-types/linux-smoke/verify/attempts/3-record-batch-compile/Cargo.lock" "$SNAP/Cargo.lock"
cp "$REPO/contracts/source/pgsql/type-conversion-v1.md" "$SNAP/contracts/source/pgsql/type-conversion-v1.md"
git -C "$REPO" rev-parse HEAD > "$EVIDENCE/linux-smoke/verify/snapshot-head.txt"
git -C "$REPO" rev-parse 'HEAD^{tree}' > "$EVIDENCE/linux-smoke/verify/snapshot-head-tree.txt"

docker volume create \
  --label oo.project=open-ontology --label oo.owner=a098 --label oo.session="$SESSION" \
  --label oo.run_id="$RUN_ID" --label oo.kind=rust \
  "$CARGO_VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology --label oo.owner=a098 --label oo.session="$SESSION" \
  --label oo.run_id="$RUN_ID" --label oo.kind=rust \
  "$TARGET_VOL" >/dev/null

docker run -d \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --init \
  --memory 8g \
  --cpus 6 \
  --pids-limit 4096 \
  --network "$NET" \
  --label oo.project=open-ontology \
  --label oo.owner=a098 \
  --label oo.session="$SESSION" \
  --label oo.run_id="$RUN_ID" \
  --label oo.kind=rust \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_TERM_COLOR=never \
  -v "$SNAP":/src \
  -v "$EVIDENCE/linux-smoke/verify/phases":/phases \
  -v "$EVIDENCE/linux-smoke/verify/inside.sh":/inside.sh:ro \
  -v "$EVIDENCE/arrow-probe":/probe-src:ro \
  -v "$ROOT/secrets":/fixture:ro \
  -v "$CARGO_VOL":/cargo \
  -v "$TARGET_VOL":/target \
  -w /src \
  "$RUST_IMAGE" \
  bash /inside.sh >/dev/null

{
  echo "run=$RUN_ID"
  echo "head=$(cat "$EVIDENCE/linux-smoke/verify/snapshot-head.txt")"
  echo "head_tree=$(cat "$EVIDENCE/linux-smoke/verify/snapshot-head-tree.txt")"
  echo "rust_image=$RUST_IMAGE"
  echo "platform=linux/arm64"
  echo "toolchain=1.94.0"
  echo "network=$NET"
  echo "rust_container=$NAME"
  echo "pg_container=oo-a098-pg-${RUN_ID}"
  echo "privileged=no"
  echo "network_mode=user_bridge_not_host"
  echo "published_ports=none"
  echo "home_mount=no"
  echo "socket_mount=no"
  echo "shared_cargo_cache=no"
  echo "pull=never"
  echo "cap_drop=ALL"
  echo "cap_add=none"
  echo "init=yes"
  echo "no_new_privileges=yes"
  echo "snapshot=$SNAP"
  echo "snapshot_method=git archive HEAD plus type overlay; Cargo.lock starts as the attempt-3 resolution of that HEAD and is committed only when this run's --locked fetch uses it"
  echo "command=bash /inside.sh"
} > "$EVIDENCE/linux-smoke/verify/invocation.txt"
docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} security={{json .HostConfig.SecurityOpt}} capdrop={{json .HostConfig.CapDrop}}' > "$EVIDENCE/linux-smoke/verify/hostconfig.txt"
echo "rust_started name=$NAME"
