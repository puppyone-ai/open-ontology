#!/bin/bash
set -eu
RUN_ID=997bbcb7-a0e9-4140-8505-e0500a317358
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
ROOT=/tmp/oo-a072-${RUN_ID}
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a072/SP04/target-fence-receipts/pre-smoke
NAME=oo-a072-msrv-${RUN_ID}
NET=oo-a072-${RUN_ID}-net
CARGO_VOL=oo-a072-${RUN_ID}-cargo
TARGET_VOL=oo-a072-${RUN_ID}-target

mkdir -p "$EVIDENCE"
running=$(docker ps --filter label=oo.project=open-ontology --filter label=oo.kind=rust-builder --format '{{.Names}}')
count=$(printf '%s\n' "$running" | sed '/^$/d' | wc -l | tr -d ' ')
if [ "$count" -ge 2 ]; then
  printf '%s\n' "$running" > "$EVIDENCE/deferred-running.txt"
  echo "deferred: two open-ontology rust builders are already running" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a072 --filter label=oo.kind=rust-builder --format '{{.Names}}')
if [ -n "$own" ]; then
  printf '%s\n' "$own" > "$EVIDENCE/deferred-own.txt"
  echo "deferred: this task already has a rust builder" >&2
  exit 3
fi

mkdir -p "$ROOT/snapshot"
# Exact committed inputs. Untracked evidence and target trees stay out.
git -C "$SRC" archive --format=tar 0bba2a7993e6b53b1bb7c587e6e978bc6f8133dc \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo \
  | tar -C "$ROOT/snapshot" -xf -
cp "$EVIDENCE/inside.sh" "$ROOT/inside.sh"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/baseline-head.txt"
git -C "$SRC" rev-parse 'HEAD^{tree}' > "$EVIDENCE/baseline-tree.txt"
git -C "$SRC" rev-parse 'HEAD^' > "$EVIDENCE/baseline-parent.txt"
git -C "$SRC" status --porcelain --untracked-files=no > "$EVIDENCE/baseline-tracked-status.txt"
find "$ROOT/snapshot" -type f | sort | shasum -a 256 > "$EVIDENCE/snapshot-files.sha256"

cat > "$EVIDENCE/invocation.txt" <<EOF
run_id=${RUN_ID}
baseline_head=0bba2a7993e6b53b1bb7c587e6e978bc6f8133dc
baseline_parent=1b22f960af6118fcdf8222f53aea6f75831c0cc9
baseline_tree=93aedfb3a6b126cbf0c9e7bb6833e41bb7d14f3e
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
snapshot=${ROOT}/snapshot
snapshot_method=git archive of the committed tree
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

echo "SNAPSHOT_DONE ${ROOT}/snapshot"
docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a072 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a072 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$NET"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a072 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a072 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a072 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a072 \
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
  --mount "type=bind,source=${ROOT}/snapshot,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a072 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a072 \
  --label oo.task=target-fence-pre-smoke \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp04-target-fence-receipts \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt"
docker cp "$NAME":/target/phases "$EVIDENCE/phases" || true
echo "docker_run_exit=${run_exit}"
exit "$run_exit"
