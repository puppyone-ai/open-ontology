#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder. Immutable archive of the fixed commit.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a163/SP07/review-pg-cli-review
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a163-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a163-builder
CARGO_VOL=oo-a163-eef831aa-cargo
TARGET_VOL=oo-a163-eef831aa-target
NET=oo-a163-eef831aa-net
FIXED=2283f904f71c9f04738a554f9c67439fa1db9bfc
PARENT=476d21c7a031c53fb8b582fb49c6a5dafd2a0b4c
BASE=3353380b03a5585a349fddd0038d0a79877c51e1
AUTHOR=aa9a083d9630bddf51d5f5fb2004b3be64a6cf6a
TREE=8e093dd327b6ba73035b1a2d0b2ea88c4b25ac42
SNAP=${ROOT}/snapshot
BEFORE=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json
AFTER=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-after.json
BEFORE_SHA=de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9
AFTER_SHA=53e2fe7e68197f8698a2a9e2683dc2131426a0faea6bfb277594f9ba9ce52903

mkdir -p "$SMOKE" "$EVIDENCE/source" "$EVIDENCE/raw" "$EVIDENCE/oracle" "$ROOT"
chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh"

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt"
grep -q 'os=linux' "$SMOKE/image.txt"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$SMOKE/containers-before.txt" || true
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a163 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$SMOKE/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "deferred: this task already has a running rust builder" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
if [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo examples/ontology-draft contracts/builder \
  | tar -x -C "$SNAP"
git -C "$SRC" archive "$FIXED" "$BEFORE" "$AFTER" | tar -x -C "$SNAP"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/src/bin/oo-builder.rs")" = "c2f1fe4a4ba11eb36263b4eeb20e17fd9a93c09d"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/tests/review_pg_cli.rs")" = "74fbe07922ab21bba4e8ab839e7b92802539acf7"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/tests/captured_pg_cli.rs")" = "9d481b1f0dd93cd7db364c32d41fadece5680fe7"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/tests/structure_draft_cli.rs")" = "1fe5e06f1fb078749cca306e8f28a9822ada957d"
test "$(git -C "$SRC" hash-object "$SNAP/contracts/builder/captured-pg-draft-v1.md")" = "0015ec4083bbbaf3287ca2baa1b887546b409955"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/src/captured_pg.rs")" = "373bcad7968d88d9be331e4622b63d1ca60b1449"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/src/draft.rs")" = "9505a8fd63e5c2517622c695d9ffeda4a639b562"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/src/lib.rs")" = "5849b4884e08ce2bdb8a553d56733748ef1715cc"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/src/pgsql.rs")" = "c7f399aa919c18858ae9d29da22deabd01ebe115"
test "$(git -C "$SRC" hash-object "$SNAP/Cargo.lock")" = "ef30377cade0ad07ba4192125f434b01452a5622"
test "$(git -C "$SRC" hash-object "$SNAP/Cargo.toml")" = "c78ee5d0575a11cd6078cf0055cdd2c673edf583"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/Cargo.toml")" = "d3cf3f73ba7884e34fd503ccd13a7d010161b78a"
cp "$EVIDENCE/oracle/a163_oracle.rs" "$SNAP/crates/oo-builder/tests/a163_oracle.rs"
before_sha=$(shasum -a 256 "$SNAP/$BEFORE" | awk '{print $1}')
after_sha=$(shasum -a 256 "$SNAP/$AFTER" | awk '{print $1}')
printf 'before=%s\nsha256=%s\nbytes=%s\nafter=%s\nsha256=%s\nbytes=%s\n' \
  "$BEFORE" "$before_sha" "$(wc -c < "$SNAP/$BEFORE" | tr -d ' ')" \
  "$AFTER" "$after_sha" "$(wc -c < "$SNAP/$AFTER" | tr -d ' ')" > "$SMOKE/fixture.txt"
test "$before_sha" = "$BEFORE_SHA"
test "$after_sha" = "$AFTER_SHA"
test "$(git -C "$SRC" rev-parse "${FIXED}^{tree}")" = "$TREE"

git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$SMOKE/source-tree.txt"
printf '%s\n' "$FIXED" > "$SMOKE/source-commit.txt"
git -C "$SRC" rev-parse "$FIXED" > "$EVIDENCE/source/commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/source/tree.txt"
git -C "$SRC" rev-parse "${FIXED}^" > "$EVIDENCE/source/parent.txt"
test "$(cat "$EVIDENCE/source/parent.txt")" = "$PARENT"
git -C "$SRC" diff --name-status "$PARENT" "$FIXED" > "$EVIDENCE/source/commit-name-status.txt"
git -C "$SRC" diff --name-status "$BASE" "$FIXED" -- \
  Cargo.toml Cargo.lock rust-toolchain.toml crates/oo-builder contracts/builder \
  > "$EVIDENCE/source/library-diff.txt"
git -C "$SRC" diff --exit-code "$BASE" "$FIXED" -- \
  Cargo.toml Cargo.lock \
  crates/oo-builder/Cargo.toml \
  crates/oo-builder/src/captured_pg.rs \
  crates/oo-builder/src/draft.rs \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/pgsql.rs
printf 'library_and_lock_diff_exit=0\n' >> "$EVIDENCE/source/library-diff.txt"
git -C "$SRC" diff -- "$PARENT" "$FIXED" -- crates/oo-builder/tests/structure_draft_cli.rs \
  > "$EVIDENCE/source/structure-draft-cli.diff"
git -C "$SRC" diff --stat "$PARENT" "$FIXED" > "$EVIDENCE/source/commit-stat.txt"
git -C "$SRC" ls-tree -r "$FIXED" -- \
  crates/oo-builder/src/bin/oo-builder.rs \
  crates/oo-builder/tests/review_pg_cli.rs \
  crates/oo-builder/tests/captured_pg_cli.rs \
  crates/oo-builder/tests/structure_draft_cli.rs \
  contracts/builder/captured-pg-draft-v1.md \
  crates/oo-builder/src/captured_pg.rs \
  crates/oo-builder/src/draft.rs \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/pgsql.rs \
  crates/oo-builder/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  > "$EVIDENCE/source/blobs.txt"
{
  echo "oracle=$(shasum -a 256 "$EVIDENCE/oracle/a163_oracle.rs" | awk '{print $1}')"
  echo "fixture_before=$before_sha"
  echo "fixture_after=$after_sha"
} > "$SMOKE/overlay.sha256"

author_root=$(mktemp -d "${ROOT}/author-manifest.XXXXXX")
git -C "$SRC" archive "$AUTHOR" \
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a161/SP07/review-pg-cli \
  | tar -x -C "$author_root"
set +e
(
  cd "$author_root/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a161/SP07/review-pg-cli"
  shasum -a 256 -c MANIFEST.sha256
) > "$EVIDENCE/source/author-manifest-check.txt" 2>&1
author_exit=$?
set -e
printf 'author_manifest_exit=%s\n' "$author_exit" >> "$EVIDENCE/source/author-manifest-check.txt"
rm -rf "$author_root"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a163 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a163 \
    "$NET" > "$SMOKE/network.txt"
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a163 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a163 --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a163 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a163 --label oo.kind=target \
  "$TARGET_VOL" >/dev/null

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  printf 'prior_status=%s\n' "$prior" > "$SMOKE/prior-container.txt"
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$SMOKE/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >> "$SMOKE/prior-container.txt"
fi

running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a163 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$SMOKE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity filled before docker run" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi

cat > "$SMOKE/invocation.txt" <<EOF
source_commit=${FIXED}
tree=${TREE}
parent=${PARENT}
fixed_base=${BASE}
author_evidence=${AUTHOR}
snapshot_method=git archive of ${FIXED}; overlay is tests/a163_oracle.rs plus the a157 catalog fixtures from that commit
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
postgres=no
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
  -e A163_ORACLE_DIR=/evidence/oracle-out \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a163 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a163 \
  --label oo.task=review-pg-cli-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  /inside.sh > "$SMOKE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$SMOKE/docker-run.exit"
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
  echo "Tmpfs=$(docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
} > "$SMOKE/hostconfig.txt"
exit "$run_exit"
