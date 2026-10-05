#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Fixed archive plus owned overlay. No PostgreSQL.
# Does not overwrite an existing console.log. Does not prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a217/SP07/reviewed-effective-oos
OVERLAY=${EVIDENCE}/linux/overlay
ROOT=/tmp/oo-a217-eef831aa-dba4-4af2-a78b-541bc9e0a217
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=c2db9c430b822a824a1062b6c3c7345924df7dd8
NAME=oo-a217-overlay
NET=oo-a217-eef831aa-net
CARGO_VOL=oo-a217-eef831aa-cargo
TARGET_VOL=oo-a217-eef831aa-target

chmod 755 "$OVERLAY/inside.sh" "$OVERLAY/launch.sh"
mkdir -p "$OVERLAY/phases" "$EVIDENCE/source/product"
if [ -f "$OVERLAY/console.log" ]; then
  echo "refusing to overwrite overlay console.log" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$OVERLAY/docker-run.exit"
  exit 1
fi
if [ ! -f "$EVIDENCE/linux/smoke/docker-run.exit" ] || [ "$(cat "$EVIDENCE/linux/smoke/docker-run.exit")" != "0" ]; then
  echo "refusing overlay before a successful smoke" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi
if docker container inspect oo-a217-smoke >/dev/null 2>&1; then
  echo "refusing overlay while the smoke container still exists" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi

git -C "$SRC" rev-parse "$FIXED" > "$OVERLAY/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$OVERLAY/fixed-tree.txt"
unstaged=$(git -C "$SRC" diff --name-only HEAD -- crates/oo-builder contracts/builder)
printf '%s\n' "$unstaged" > "$OVERLAY/unstaged.txt"
if [ "$unstaged" != "crates/oo-builder/src/lib.rs" ]; then
  echo "refusing uncommitted tracked drift: ${unstaged}" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi
staged=$(git -C "$SRC" diff --cached --name-only -- crates/oo-builder contracts/builder)
if [ -n "$staged" ]; then
  echo "refusing staged drift: ${staged}" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi
git -C "$SRC" diff "$FIXED" -- crates/oo-builder/src/lib.rs > "$OVERLAY/lib.rs.diff"
added_mods=$(grep -E '^\+mod ' "$OVERLAY/lib.rs.diff" || true)
if [ "$added_mods" != "+mod reviewed_effective;" ]; then
  echo "lib.rs diff adds an unexpected module" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi

mkdir -p "$ROOT"
rm -rf "$ROOT/overlay"
mkdir -p "$ROOT/overlay"
FIXTURE=evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  "$FIXTURE/catalog-before.json" \
  "$FIXTURE/catalog-after.json" \
  | tar -x -C "$ROOT/overlay"
if [ ! -f "$ROOT/overlay/crates/oo-builder/src/lib.rs" ]; then
  echo "archive missing oo-builder lib.rs" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/overlay/crates/oo-builder/src/reviewed_effective.rs" ]; then
  echo "fixed archive already contains the product module" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi

cp "$SRC/crates/oo-builder/src/lib.rs" "$ROOT/overlay/crates/oo-builder/src/lib.rs"
cp "$SRC/crates/oo-builder/src/reviewed_effective.rs" "$ROOT/overlay/crates/oo-builder/src/reviewed_effective.rs"
cp "$SRC/crates/oo-builder/src/bin/reviewed-effective-oos.rs" "$ROOT/overlay/crates/oo-builder/src/bin/reviewed-effective-oos.rs"
cp "$SRC/crates/oo-builder/tests/reviewed_effective.rs" "$ROOT/overlay/crates/oo-builder/tests/reviewed_effective.rs"
cp "$SRC/contracts/builder/reviewed-effective-oos-v1.md" "$ROOT/overlay/contracts/builder/reviewed-effective-oos-v1.md"
cp "$SRC/crates/oo-builder/src/lib.rs" "$EVIDENCE/source/product/lib.rs"
cp "$SRC/crates/oo-builder/src/reviewed_effective.rs" "$EVIDENCE/source/product/reviewed_effective.rs"
cp "$SRC/crates/oo-builder/src/bin/reviewed-effective-oos.rs" "$EVIDENCE/source/product/reviewed-effective-oos.rs"
cp "$SRC/crates/oo-builder/tests/reviewed_effective.rs" "$EVIDENCE/source/product/reviewed_effective_test.rs"
cp "$SRC/contracts/builder/reviewed-effective-oos-v1.md" "$EVIDENCE/source/product/reviewed-effective-oos-v1.md"

blob_ok=0
{
  while IFS='|' read -r path name; do
    [ -n "$path" ] || continue
    want=$(git -C "$SRC" rev-parse "$FIXED:$path")
    got=$(git -C "$SRC" hash-object "$ROOT/overlay/$path")
    printf '%s %s want=%s got=%s\n' "$path" "$name" "$want" "$got"
    if [ "$want" != "$got" ]; then
      echo "blob mismatch $path" >&2
      blob_ok=1
    fi
  done <<'BLOBS'
crates/oo-builder/src/bin/oo-builder.rs|oo-builder.rs
crates/oo-builder/src/role_pack.rs|role_pack.rs
crates/oo-builder/src/role_pack_cli.rs|role_pack_cli.rs
crates/oo-builder/src/captured_pg.rs|captured_pg.rs
crates/oo-builder/src/canonical.rs|canonical.rs
crates/oo-builder/Cargo.toml|oo-builder-Cargo.toml
Cargo.lock|Cargo.lock
Cargo.toml|Cargo.toml
BLOBS
} > "$OVERLAY/unchanged-blobs.txt"
if [ "$blob_ok" -ne 0 ]; then
  echo "archive blob mismatch" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi
git -C "$SRC" show "$FIXED:rust-toolchain.toml" > "$OVERLAY/rust-toolchain.toml.fixed"
mv "$ROOT/overlay/rust-toolchain.toml" "$OVERLAY/rust-toolchain.toml.aside"
printf '%s\n' "moved aside; pinned image stays 1.94.0; git object unchanged" > "$OVERLAY/toolchain-aside.txt"
printf '%s\n' "overlay=owned archive=$FIXED" > "$OVERLAY/overlay-source.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a217 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a217 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$OVERLAY/network.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$OVERLAY/host-cargo-procs.txt" || true
host_n=$(grep -c . "$OVERLAY/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$OVERLAY/builders-before.txt" || true
builder_n=$(grep -c . "$OVERLAY/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$OVERLAY/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$OVERLAY/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a217 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$OVERLAY/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$OVERLAY/image.txt"
grep -q 'arch=arm64' "$OVERLAY/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a217 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a217 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a217 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a217 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
printf '%s\n' "$CARGO_VOL" > "$OVERLAY/volume-cargo.txt"
printf '%s\n' "$TARGET_VOL" > "$OVERLAY/volume-target.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named overlay container already exists" >&2
  printf '%s\n' 2 > "$OVERLAY/docker-run.exit"
  exit 2
fi

cat > "$OVERLAY/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a217
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
cargo_volume=${CARGO_VOL}
target_volume=${TARGET_VOL}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
postgres=none
phase=overlay
memory=6g
cpus=1
global_cap=10
archive=${ROOT}/overlay
fixed=${FIXED}
EOF

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/overlay,target=/src,readonly" \
  --mount "type=bind,source=${OVERLAY}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a217 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a217 \
  --label oo.task=reviewed-effective-oos-overlay \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$OVERLAY/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$OVERLAY/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "CapAdd=$(docker inspect -f '{{json .HostConfig.CapAdd}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME")"
  echo "Tmpfs=$(docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$OVERLAY/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$OVERLAY/mounts.txt"
docker rm "$NAME" > "$OVERLAY/container-removed.txt"
exit "$run_exit"
