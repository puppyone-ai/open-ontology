#!/bin/bash
# One owned linux/arm64 Rust 1.94 builder. Immutable archive of the fixed commit.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a160/SP07/captured-pg-cli-review
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a160-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a160-builder
CARGO_VOL=oo-a160-eef831aa-cargo
TARGET_VOL=oo-a160-eef831aa-target
NET=oo-a160-eef831aa-net
FIXED=3353380b03a5585a349fddd0038d0a79877c51e1
ACCEPTED=63466471a6d403e0a2a29c8d1ae064179da65bd8
SNAP=${ROOT}/snapshot
FIXTURE=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out/catalog-before.json
FIXTURE_SHA=de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9

mkdir -p "$SMOKE" "$EVIDENCE/source" "$EVIDENCE/raw" "$ROOT"
chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh"

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt"
grep -q 'os=linux' "$SMOKE/image.txt"

docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}' > "$SMOKE/containers-before.txt" || true
running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a160 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
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

actual=$(shasum -a 256 "$SRC/$FIXTURE" | awk '{print $1}')
printf 'path=%s\nsha256=%s\nbytes=%s\na157_commit=92829c3b1b80abc10324535051ca5c84faa0d2d8\n' "$FIXTURE" "$actual" "$(wc -c < "$SRC/$FIXTURE" | tr -d ' ')" > "$SMOKE/fixture.txt"
test "$actual" = "$FIXTURE_SHA"

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo examples/ontology-draft contracts/builder \
  | tar -x -C "$SNAP"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/src/bin/oo-builder.rs")" = "327eaa1519380da3b22c724617f3b76f0ff46736"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/tests/captured_pg_cli.rs")" = "d51f3baae9e98812e4a72cd29c395fc90bd25c93"
test "$(git -C "$SRC" hash-object "$SNAP/contracts/builder/captured-pg-draft-v1.md")" = "d59febd6d8034983ce841f45b1bd8e27d6560cbd"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/src/captured_pg.rs")" = "373bcad7968d88d9be331e4622b63d1ca60b1449"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/src/draft.rs")" = "9505a8fd63e5c2517622c695d9ffeda4a639b562"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/src/lib.rs")" = "5849b4884e08ce2bdb8a553d56733748ef1715cc"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/src/pgsql.rs")" = "c7f399aa919c18858ae9d29da22deabd01ebe115"
test "$(git -C "$SRC" hash-object "$SNAP/Cargo.lock")" = "ef30377cade0ad07ba4192125f434b01452a5622"
test "$(git -C "$SRC" hash-object "$SNAP/Cargo.toml")" = "c78ee5d0575a11cd6078cf0055cdd2c673edf583"
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-builder/Cargo.toml")" = "d3cf3f73ba7884e34fd503ccd13a7d010161b78a"
cp "$EVIDENCE/oracle/a160_oracle.rs" "$SNAP/crates/oo-builder/tests/a160_oracle.rs"
mkdir -p "$SNAP/$(dirname "$FIXTURE")"
cp "$SRC/$FIXTURE" "$SNAP/$FIXTURE"
test -f "$SNAP/crates/oo-builder/tests/a160_oracle.rs"
test -f "$SNAP/examples/ontology-draft/goldens/customer.draft.json"
test -f "$SNAP/$FIXTURE"

git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$SMOKE/source-tree.txt"
printf '%s\n' "$FIXED" > "$SMOKE/source-commit.txt"
git -C "$SRC" rev-parse "$FIXED" > "$EVIDENCE/source/commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/source/tree.txt"
git -C "$SRC" rev-parse "${FIXED}^" > "$EVIDENCE/source/parent.txt"
git -C "$SRC" diff --name-status "$ACCEPTED" "$FIXED" -- \
  Cargo.toml Cargo.lock rust-toolchain.toml \
  crates/oo-builder crates/oo-core/Cargo.toml crates/oo-connectors/Cargo.toml \
  contracts/builder/captured-pg-draft-v1.md \
  > "$EVIDENCE/source/library-diff.txt"
git -C "$SRC" diff --exit-code "$ACCEPTED" "$FIXED" -- \
  Cargo.toml Cargo.lock \
  crates/oo-builder/Cargo.toml \
  crates/oo-builder/src/captured_pg.rs \
  crates/oo-builder/src/draft.rs \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/pgsql.rs \
  crates/oo-core \
  crates/oo-connectors/Cargo.toml
printf 'library_and_lock_diff_exit=0\n' >> "$EVIDENCE/source/library-diff.txt"
git -C "$SRC" ls-tree -r "$FIXED" -- \
  crates/oo-builder/src/bin/oo-builder.rs \
  crates/oo-builder/tests/captured_pg_cli.rs \
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
  echo "oracle=$(shasum -a 256 "$EVIDENCE/oracle/a160_oracle.rs" | awk '{print $1}')"
  echo "fixture=$actual"
} > "$SMOKE/overlay.sha256"
cmp "$EVIDENCE/oracle/a160_oracle.rs" "$SNAP/crates/oo-builder/tests/a160_oracle.rs"

author_root=$(mktemp -d "${ROOT}/author-manifest.XXXXXX")
git -C "$SRC" archive 2996562ceb332cc0ce505c703e775615fa321e5b \
  evidence/eef831aa-dba4-4af2-a78b-541bc9e0a158/SP07/captured-pg-cli \
  | tar -x -C "$author_root"
(
  cd "$author_root/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a158/SP07/captured-pg-cli"
  shasum -a 256 -c MANIFEST.sha256
) > "$EVIDENCE/source/author-manifest-check.txt"
rm -rf "$author_root"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a160 \
    --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a160 \
    "$NET" > "$SMOKE/network.txt"
fi
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a160 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a160 --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a160 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a160 --label oo.kind=target \
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

cat > "$SMOKE/invocation.txt" <<EOF
source_commit=${FIXED}
tree=$(cat "$SMOKE/source-tree.txt")
accepted_api=${ACCEPTED}
snapshot_method=git archive of ${FIXED}; overlay is tests/a160_oracle.rs plus the a157 catalog fixture
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
  -e A160_ORACLE_DIR=/evidence/oracle-out \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a160 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a160 \
  --label oo.task=captured-pg-cli-review \
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
} > "$SMOKE/hostconfig.txt"
exit "$run_exit"
