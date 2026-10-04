#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder. Fixed archive plus the a161 overlay.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a161/SP07/review-pg-cli/linux/focused
ROOT=/tmp/oo-a161-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a161-builder-focused
CARGO_VOL=oo-a161-eef831aa-cargo
TARGET_VOL=oo-a161-eef831aa-target
NET=oo-a161-eef831aa-net
FIXED=3353380b03a5585a349fddd0038d0a79877c51e1
SNAP=${ROOT}/focused-snapshot
BEFORE=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json
AFTER=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-after.json
BEFORE_SHA=de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9
AFTER_SHA=53e2fe7e68197f8698a2a9e2683dc2131426a0faea6bfb277594f9ba9ce52903

mkdir -p "$EVIDENCE" "$ROOT"
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
grep -q 'os=linux' "$EVIDENCE/image.txt"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$EVIDENCE/containers-before.txt" || true
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a161 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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

before_actual=$(shasum -a 256 "$SRC/$BEFORE" | awk '{print $1}')
after_actual=$(shasum -a 256 "$SRC/$AFTER" | awk '{print $1}')
printf 'before=%s\nbefore_sha256=%s\nafter=%s\nafter_sha256=%s\n' "$BEFORE" "$before_actual" "$AFTER" "$after_actual" > "$EVIDENCE/fixture.txt"
test "$before_actual" = "$BEFORE_SHA"
test "$after_actual" = "$AFTER_SHA"

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo examples/ontology-draft contracts/builder \
  | tar -x -C "$SNAP"
cp "$SRC/crates/oo-builder/src/bin/oo-builder.rs" "$SNAP/crates/oo-builder/src/bin/oo-builder.rs"
cp "$SRC/crates/oo-builder/tests/review_pg_cli.rs" "$SNAP/crates/oo-builder/tests/review_pg_cli.rs"
cp "$SRC/crates/oo-builder/tests/captured_pg_cli.rs" "$SNAP/crates/oo-builder/tests/captured_pg_cli.rs"
cp "$SRC/crates/oo-builder/tests/structure_draft_cli.rs" "$SNAP/crates/oo-builder/tests/structure_draft_cli.rs"
cp "$SRC/contracts/builder/captured-pg-draft-v1.md" "$SNAP/contracts/builder/captured-pg-draft-v1.md"
mkdir -p "$SNAP/$(dirname "$BEFORE")"
cp "$SRC/$BEFORE" "$SNAP/$BEFORE"
cp "$SRC/$AFTER" "$SNAP/$AFTER"
test -f "$SNAP/crates/oo-builder/tests/review_pg_cli.rs"
test -f "$SNAP/$BEFORE"
test -f "$SNAP/$AFTER"

git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/source-tree.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/source-commit.txt"
{
  echo "bin=$(shasum -a 256 "$SRC/crates/oo-builder/src/bin/oo-builder.rs" | awk '{print $1}')"
  echo "review_test=$(shasum -a 256 "$SRC/crates/oo-builder/tests/review_pg_cli.rs" | awk '{print $1}')"
  echo "captured_test=$(shasum -a 256 "$SRC/crates/oo-builder/tests/captured_pg_cli.rs" | awk '{print $1}')"
  echo "structure_test=$(shasum -a 256 "$SRC/crates/oo-builder/tests/structure_draft_cli.rs" | awk '{print $1}')"
  echo "contract=$(shasum -a 256 "$SRC/contracts/builder/captured-pg-draft-v1.md" | awk '{print $1}')"
  echo "before=$before_actual"
  echo "after=$after_actual"
} > "$EVIDENCE/overlay.sha256"
cmp "$SRC/crates/oo-builder/src/bin/oo-builder.rs" "$SNAP/crates/oo-builder/src/bin/oo-builder.rs"
cmp "$SRC/crates/oo-builder/tests/review_pg_cli.rs" "$SNAP/crates/oo-builder/tests/review_pg_cli.rs"
cmp "$SRC/crates/oo-builder/tests/captured_pg_cli.rs" "$SNAP/crates/oo-builder/tests/captured_pg_cli.rs"
cmp "$SRC/crates/oo-builder/tests/structure_draft_cli.rs" "$SNAP/crates/oo-builder/tests/structure_draft_cli.rs"
cmp "$SRC/contracts/builder/captured-pg-draft-v1.md" "$SNAP/contracts/builder/captured-pg-draft-v1.md"

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
source_commit=${FIXED}
tree=$(cat "$EVIDENCE/source-tree.txt")
snapshot_method=git archive of ${FIXED} plus overlay of oo-builder.rs, review_pg_cli.rs, captured_pg_cli.rs, structure_draft_cli.rs, captured-pg-draft-v1.md, and the a157 catalog-before and catalog-after fixtures
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
network=${NET}
pull=never
privileged=no
host_network=no
docker_socket=no
home_mount=no
phase=focused CLI
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
  --entrypoint bash \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a161 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a161 \
  --label oo.task=review-pg-cli-focused \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  /inside.sh > "$EVIDENCE/console.log" 2>&1
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
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
exit "$run_exit"
