#!/bin/bash
# RED run. HEAD archive plus the unfixed buffered.rs (panic expects kept) and the new tests.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a129/SP03/view-shrink/red
ROOT=/tmp/oo-a129-eef831aa
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a129-view-shrink-red
NET=oo-a129-eef831aa-net
CARGO_VOL=oo-a129-eef831aa-cargo
TARGET_VOL=oo-a129-eef831aa-target
COMMIT=$(git -C "$SRC" rev-parse HEAD)
TREE=$(git -C "$SRC" rev-parse 'HEAD^{tree}')
PARENT=$(git -C "$SRC" rev-parse 'HEAD^')

mkdir -p "$EVIDENCE" "$ROOT"
cp "$EVIDENCE/inside.sh" "$ROOT/inside-red.sh"
chmod 755 "$ROOT/inside-red.sh" "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

running_rust=$(docker ps --format '{{.ID}} {{.Image}} {{.Label "oo.kind"}}' | awk '$2 ~ /rust/ || $3=="rust-builder" {c++} END{print c+0}')
own=$(docker ps -q --filter label=oo.owner=a129 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'running_rust_image_or_label=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

git -C "$SRC" rev-parse HEAD > "$EVIDENCE/source-head.txt"
git -C "$SRC" rev-parse 'HEAD^{tree}' > "$EVIDENCE/source-tree.txt"
git -C "$SRC" rev-parse 'HEAD^' > "$EVIDENCE/source-parent.txt"
test "$(cat "$EVIDENCE/source-head.txt")" = "$COMMIT"
test "$(cat "$EVIDENCE/source-tree.txt")" = "$TREE"
test "$(cat "$EVIDENCE/source-parent.txt")" = "$PARENT"

rm -rf "$ROOT/red-snapshot"
mkdir -p "$ROOT/red-snapshot"
git -C "$SRC" archive --format=tar HEAD \
  Cargo.toml Cargo.lock rust-toolchain.toml crates .cargo \
  contracts/source/pgsql/empty-catalog.json \
  | tar -C "$ROOT/red-snapshot" -xf -
overlay() {
  rel=$1
  mkdir -p "$ROOT/red-snapshot/$(dirname "$rel")"
  cp "$SRC/$rel" "$ROOT/red-snapshot/$rel"
  cmp -s "$SRC/$rel" "$ROOT/red-snapshot/$rel"
}
overlay crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
overlay crates/oo-connectors/tests/pg_wire_view_shrink.rs
grep -q 'incoming view shrank during advance' "$ROOT/red-snapshot/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs"
(
  cd "$SRC"
  shasum -a 256 \
    crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs \
    crates/oo-connectors/tests/pg_wire_view_shrink.rs \
    crates/oo-connectors/vendor/PATCH.md
) > "$EVIDENCE/red-source.sha256"
printf '%s\n' 0 > "$EVIDENCE/snapshot-prep.exit"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker network inspect "$NET" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null

if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "deferred: named container is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${COMMIT}
source_parent=${PARENT}
source_tree=${TREE}
overlay=buffered.rs with panic expects kept plus tests/pg_wire_view_shrink.rs
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
phase=red-unfixed
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
  --mount "type=bind,source=${ROOT}/red-snapshot,target=/src" \
  --mount "type=bind,source=${ROOT}/inside-red.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a129 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a129 \
  --label oo.task=view-shrink-red \
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
} > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" > "$EVIDENCE/container-rm.txt"
exit "$run_exit"
