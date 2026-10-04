#!/bin/bash
# Immutable archive of fixed commit bedd6a2c. One rust builder. No overlay. No product edits. No Postgres.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a155/SP03/reader-docs/linux/smoke
ROOT=/tmp/oo-a155-eef831aa-dba4-4af2-a78b-541bc9e0a155
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a155-smoke
NET=oo-a155-eef831aa-net
CARGO_VOL=oo-a155-eef831aa-cargo
TARGET_VOL=oo-a155-eef831aa-target
FIXED=bedd6a2ced65edf6a44ac85ecd40551e1b79a45c
EXPECT_TREE=10a29baf168403bc849e9fd2078a16f1d1ee0682
SNAP=${ROOT}/smoke-src

mkdir -p "$EVIDENCE" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  exit 2
fi
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
docker ps --format '{{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}} {{.Status}}' > "$EVIDENCE/containers-before.txt"
head_now=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$head_now" > "$EVIDENCE/workspace-head.txt"
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a155 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a155 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a155 \
    "$NET" > "$EVIDENCE/network.txt"
fi
actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
test "$actual_tree" = "$EXPECT_TREE"
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
file_sha() { shasum -a 256 "$1" | awk '{print $1}'; }
file_sha "$SNAP/crates/oo-connectors/src/model.rs" > "$EVIDENCE/expect-model.sha256"
file_sha "$SNAP/crates/oo-connectors/src/types/mod.rs" > "$EVIDENCE/expect-types.sha256"
file_sha "$SNAP/crates/oo-connectors/src/types/tests.rs" > "$EVIDENCE/expect-types-tests.sha256"
file_sha "$SNAP/crates/oo-connectors/src/key_plan.rs" > "$EVIDENCE/expect-keyplan.sha256"
file_sha "$SNAP/crates/oo-connectors/src/lib.rs" > "$EVIDENCE/expect-lib.sha256"
file_sha "$SNAP/crates/oo-connectors/src/snapshot.rs" > "$EVIDENCE/expect-snapshot.sha256"
file_sha "$SNAP/crates/oo-connectors/src/stream.rs" > "$EVIDENCE/expect-stream.sha256"
file_sha "$SNAP/crates/oo-connectors/src/session.rs" > "$EVIDENCE/expect-session.sha256"
file_sha "$SNAP/crates/oo-connectors/src/sql.rs" > "$EVIDENCE/expect-sql.sha256"
file_sha "$SNAP/crates/oo-connectors/Cargo.toml" > "$EVIDENCE/expect-cargo-toml.sha256"
file_sha "$SNAP/Cargo.lock" > "$EVIDENCE/expect-cargo-lock.sha256"
file_sha "$SNAP/contracts/source/pgsql/reader-lifecycle-v1.md" > "$EVIDENCE/expect-lifecycle.sha256"
file_sha "$SNAP/contracts/source/pgsql/bounded-stream-v1.md" > "$EVIDENCE/expect-bounded.sha256"
file_sha "$SNAP/crates/oo-connectors/README.md" > "$EVIDENCE/expect-crate-readme.sha256"
file_sha "$SNAP/contracts/source/pgsql/README.md" > "$EVIDENCE/expect-catalog-readme.sha256"
file_sha "$SNAP/contracts/source/pgsql/reader-keys-v1.md" > "$EVIDENCE/expect-reader-keys.sha256"
file_sha "$SNAP/contracts/source/pgsql/type-conversion-v1.md" > "$EVIDENCE/expect-type-conversion.sha256"
file_sha "$SNAP/contracts/source/pgsql/snapshot-owner-v1.md" > "$EVIDENCE/expect-snapshot-owner.sha256"
blob() { git -C "$SRC" rev-parse "${FIXED}:$1"; }
{
  echo "model $(blob crates/oo-connectors/src/model.rs)"
  echo "types_mod $(blob crates/oo-connectors/src/types/mod.rs)"
  echo "types_tests $(blob crates/oo-connectors/src/types/tests.rs)"
  echo "key_plan $(blob crates/oo-connectors/src/key_plan.rs)"
  echo "lib $(blob crates/oo-connectors/src/lib.rs)"
  echo "snapshot $(blob crates/oo-connectors/src/snapshot.rs)"
  echo "stream $(blob crates/oo-connectors/src/stream.rs)"
  echo "session $(blob crates/oo-connectors/src/session.rs)"
  echo "sql $(blob crates/oo-connectors/src/sql.rs)"
  echo "cargo_toml $(blob crates/oo-connectors/Cargo.toml)"
  echo "cargo_lock $(blob Cargo.lock)"
  echo "lifecycle $(blob contracts/source/pgsql/reader-lifecycle-v1.md)"
  echo "bounded $(blob contracts/source/pgsql/bounded-stream-v1.md)"
  echo "crate_readme $(blob crates/oo-connectors/README.md)"
  echo "catalog_readme $(blob contracts/source/pgsql/README.md)"
  echo "reader_keys $(blob contracts/source/pgsql/reader-keys-v1.md)"
  echo "type_conversion $(blob contracts/source/pgsql/type-conversion-v1.md)"
  echo "snapshot_owner $(blob contracts/source/pgsql/snapshot-owner-v1.md)"
  echo "tree ${actual_tree}"
  echo "parent $(git -C "$SRC" rev-parse "${FIXED}^")"
  echo "reader_api $(git -C "$SRC" rev-parse 16ca2969cd449653a64d98d9831d3c8b8976942a)"
  echo "activation_review $(git -C "$SRC" rev-parse b476c7561f7507b8f17128407a257907172b6f9a)"
} > "$EVIDENCE/fixed-blobs.txt"
awk '
  $1=="model" && $2!="5964e40036b76efe561c7441b6acc0f05333265a" {bad=1}
  $1=="types_mod" && $2!="dbb95bcc8980b68d66502300fc13e4bfcc800b72" {bad=1}
  $1=="types_tests" && $2!="3e98361d79bd6bb1deb8b1700e5820a0f2410d88" {bad=1}
  $1=="key_plan" && $2!="6052fe9ff1bc4bdc0a3e0e3b6fd9c717649aea86" {bad=1}
  $1=="lib" && $2!="64a7efae0e442e7babf67a56ba07d398749ef5f0" {bad=1}
  $1=="snapshot" && $2!="10176c16a64f45409063a1236c0651065da8a05d" {bad=1}
  $1=="stream" && $2!="5deb30e5da09e58904d04cce22760c5ffc284f96" {bad=1}
  $1=="session" && $2!="7e191eacbf61439680cbd92a7109ded6c8a05596" {bad=1}
  $1=="sql" && $2!="6badaf187e33346c1657c632e6d5c43fe90f1ea2" {bad=1}
  $1=="cargo_toml" && $2!="e8e3254da87a29ce90c485e8ad6566dbd4654f43" {bad=1}
  $1=="cargo_lock" && $2!="ef30377cade0ad07ba4192125f434b01452a5622" {bad=1}
  $1=="lifecycle" && $2!="6f4f242f3cc00c2149bde91c2b782a72c0365813" {bad=1}
  $1=="bounded" && $2!="1226ea542bec16c64fd02b99e92318eb49f1bc58" {bad=1}
  $1=="crate_readme" && $2!="55d104709504b18d0e3f82b24a591e983c16a1fe" {bad=1}
  $1=="catalog_readme" && $2!="88c1947e1685caa036c07c7dc609f9b2b38443f2" {bad=1}
  $1=="reader_keys" && $2!="346833019418700f39e3cc3061d74dda4dbcabd8" {bad=1}
  $1=="type_conversion" && $2!="a86eb4595a3d71743eb2a7fd81e9e48dcfcf20ae" {bad=1}
  $1=="snapshot_owner" && $2!="724c692bc138732500dc01d44aa0fe3e0a97296d" {bad=1}
  $1=="tree" && $2!="10a29baf168403bc849e9fd2078a16f1d1ee0682" {bad=1}
  END { if (bad) exit 2 }
' "$EVIDENCE/fixed-blobs.txt"
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a155 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a155 \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a155 --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a155 \
  "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cat > "$EVIDENCE/invocation.txt" <<'EOF'
cargo fetch --manifest-path Cargo.toml --locked
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
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
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a155 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a155 \
  --label oo.task=reader-docs-smoke \
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
