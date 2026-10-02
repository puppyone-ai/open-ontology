#!/bin/bash
# One owned Linux arm64 Rust 1.94 builder for the fixed segment correction.
# The snapshot is git archive of fc877c4 plus the owned product files only.
# Does not pull, prune, or touch foreign containers.
set -eu
RUN_ID=51b9fcf0-eb99-4b82-a8f8-f914415571cf
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a092-eef831aa-dba4-4af2-a78b-541bc9e0a092
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a092/SP04/segment-null-correction/linux/fixed
REPO=/Users/redux/puppy-one/open-ontology
NAME=oo-a092-msrv-${RUN_ID}
NET=oo-a092-${RUN_ID}-net
CARGO_VOL=oo-a092-${RUN_ID}-cargo
TARGET_VOL=oo-a092-${RUN_ID}-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a092
SOURCE=fc877c482a61729ea46e779a674a46f8f72c80c5

mkdir -p "$EVIDENCE" "$ROOT"
chmod +x "$EVIDENCE/inside.sh"

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host_cargo_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/ {print}' || true
  echo "rust_builder_labels:"
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}} {{.Status}} {{.Label "oo.owner"}}' || true
} > "$EVIDENCE/builders-before.txt"

running=$(docker ps --filter label=oo.project=open-ontology --filter label=oo.kind=rust-builder --format '{{.Names}}')
count=$(printf '%s\n' "$running" | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -ax -o comm= | awk '$1 ~ /^(cargo|rustc|rustdoc|clippy-driver)$/' | wc -l | tr -d ' ')
printf 'builder_containers=%s host_cargo=%s\n' "$count" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$((count + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builders already at global max" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a092 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  exit 1
fi

rm -rf "$ROOT/fixed-snapshot"
mkdir -p "$ROOT/fixed-snapshot"
git -C "$REPO" archive "$SOURCE" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  contracts/pipeline/target-fence-receipts \
  | tar -x -C "$ROOT/fixed-snapshot"
overlay='crates/oo-pipeline/migrations/0002_target_segment_null_expand.sql
crates/oo-pipeline/migrations/README.md
crates/oo-pipeline/tests/segment_null_correction.rs
contracts/pipeline/target-fence-receipts/README.md
contracts/pipeline/target-fence-receipts/target-fence-receipts-v2.json'
printf '%s\n' "$overlay" > "$EVIDENCE/overlay.txt"
while IFS= read -r rel; do
  [ -n "$rel" ] || continue
  cp "$REPO/$rel" "$ROOT/fixed-snapshot/$rel"
done <<EOF
$overlay
EOF
git -C "$REPO" rev-parse "$SOURCE" > "$EVIDENCE/source-commit.txt"
git -C "$REPO" rev-parse "$SOURCE^{tree}" > "$EVIDENCE/source-tree.txt"
git -C "$REPO" rev-parse "$SOURCE^" > "$EVIDENCE/source-parent.txt"
find "$ROOT/fixed-snapshot" -type f | sort | shasum -a 256 > "$EVIDENCE/snapshot.sha256"

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
source_commit=${SOURCE}
source_tree=$(cat "$EVIDENCE/source-tree.txt")
source_parent=$(cat "$EVIDENCE/source-parent.txt")
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/fixed-snapshot
snapshot_method=git archive of ${SOURCE} plus owned overlay listed in overlay.txt
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
cap_drop=ALL
cap_add=none
init=yes
no_new_privileges=yes
command=cargo fetch --manifest-path Cargo.toml --locked; cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline; cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings
EOF

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a092 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$NET"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a092 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a092 \
  --label oo.session="${SESSION}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null

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
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${ROOT}/fixed-snapshot,target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a092 \
  --label oo.session="${SESSION}" \
  --label oo.task=segment-null-fixed \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp04-segment-null \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt"
docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} readonly_root={{.HostConfig.ReadonlyRootfs}}' > "$EVIDENCE/hostconfig.txt"
docker inspect "$NAME" --format '{{json .HostConfig.Binds}}{{json .HostConfig.Mounts}}' > "$EVIDENCE/mounts.txt"
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
