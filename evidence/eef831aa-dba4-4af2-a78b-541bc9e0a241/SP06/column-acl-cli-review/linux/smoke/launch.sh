#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Immutable archive of 13566602. No PostgreSQL.
# Origin: a235 smoke launch.sh af73d847fd69c8510fa99b4c01ec0eac7e094a28
# and a204 preoverlay launch.sh 3737c1a8321814ff3b1070ddf8b89d6864c63f6f.
# Changes: owner a241, fixed 13566602, the committed CLI files must already be present,
# the review test must be absent, 6GiB, 1 CPU, cap 10.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a241/SP06/column-acl-cli-review
SMOKE=${EVIDENCE}/linux/smoke
ROOT=/tmp/oo-a241-eef831aa-dba4-4af2-a78b-541bc9e0a241
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=13566602dc36ce6ebd03fef25a9bd17aae7a281c
NAME=oo-a241-smoke
NET=oo-a241-eef831aa-net
CARGO_VOL=oo-a241-eef831aa-cargo
TARGET_VOL=oo-a241-eef831aa-target

chmod 755 "$SMOKE/inside.sh" "$SMOKE/launch.sh" "$SMOKE/extract-libclang.sh"
mkdir -p "$SMOKE/phases" "$EVIDENCE/source/blobs"
if [ -f "$SMOKE/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$SMOKE/docker-run.exit"
  exit 1
fi

git -C "$SRC" rev-parse "$FIXED" > "$SMOKE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$SMOKE/fixed-tree.txt"
git -C "$SRC" rev-parse "${FIXED}^" > "$EVIDENCE/source/PARENT"
printf '%s\n' "$FIXED" > "$EVIDENCE/source/COMMIT"
mkdir -p "$ROOT"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/bin/install-column-acl.rs" ]; then
  echo "archive is missing install-column-acl.rs" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/tests/install_column_acl_cli.rs" ]; then
  echo "archive is missing the committed cli test" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/tests/install_column_acl_a241_review.rs" ]; then
  echo "archive already contains the review test" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
want_files=$(git -C "$SRC" ls-tree -r --name-only "$FIXED" -- \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts | wc -l | awk '{print $1}')
got_files=$(find "$ROOT/archive" -type f | wc -l | awk '{print $1}')
printf 'listed=%s extracted=%s\n' "$want_files" "$got_files" > "$SMOKE/archive-count.txt"
if [ "$want_files" != "$got_files" ]; then
  echo "archive file count mismatch" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
git -C "$SRC" show "$FIXED:rust-toolchain.toml" > "$SMOKE/rust-toolchain.toml.fixed"
mv "$ROOT/archive/rust-toolchain.toml" "$SMOKE/rust-toolchain.toml.aside"
printf '%s\n' "moved aside; pinned image stays 1.94.0; git object unchanged" > "$SMOKE/toolchain-aside.txt"
blob_ok=0
{
  while IFS='|' read -r path name; do
    [ -n "$path" ] || continue
    want=$(git -C "$SRC" rev-parse "$FIXED:$path")
    got=$(git -C "$SRC" hash-object "$ROOT/archive/$path")
    printf '%s %s want=%s got=%s\n' "$path" "$name" "$want" "$got"
    if [ "$want" != "$got" ]; then
      echo "blob mismatch $path" >&2
      blob_ok=1
    fi
    cp "$ROOT/archive/$path" "$EVIDENCE/source/blobs/$name"
  done <<'BLOBS'
crates/oo-gateway/src/bin/install-column-acl.rs|install-column-acl.rs
crates/oo-gateway/tests/install_column_acl_cli.rs|install_column_acl_cli.rs
contracts/gateway/install-column-acl-cli-v1.md|install-column-acl-cli-v1.md
crates/oo-gateway/src/policy.rs|policy.rs
crates/oo-gateway/src/lib.rs|lib.rs
crates/oo-gateway/Cargo.toml|oo-gateway-Cargo.toml
Cargo.lock|Cargo.lock
contracts/gateway/column-acl-set-v1.md|column-acl-set-v1.md
contracts/gateway/reviewed-policy-v1.md|reviewed-policy-v1.md
crates/oo-gateway/src/bin/query-pg.rs|query-pg.rs
crates/oo-gateway/src/bin/reviewed-policy.rs|reviewed-policy.rs
BLOBS
} > "$SMOKE/archive-blobs.txt"
want_tool=$(git -C "$SRC" rev-parse "$FIXED:rust-toolchain.toml")
got_tool=$(git -C "$SRC" hash-object "$SMOKE/rust-toolchain.toml.aside")
printf 'rust-toolchain.toml toolchain want=%s got=%s\n' "$want_tool" "$got_tool" >> "$SMOKE/archive-blobs.txt"
if [ "$want_tool" != "$got_tool" ]; then
  blob_ok=1
fi
if [ "$blob_ok" -ne 0 ]; then
  echo "archive blob mismatch" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi
printf '%s\n' "overlay=absent archive=$FIXED" > "$SMOKE/overlay-absent.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a241 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a241 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$SMOKE/network.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$SMOKE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$SMOKE/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$SMOKE/builders-before.txt" || true
builder_n=$(grep -c . "$SMOKE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$SMOKE/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a241 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$SMOKE/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$SMOKE/image.txt"
grep -q 'arch=arm64' "$SMOKE/image.txt" || {
  echo "pinned rust image is not arm64" >&2
  printf '%s\n' 1 > "$SMOKE/docker-run.exit"
  exit 1
}
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a241 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a241 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a241 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a241 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
printf '%s\n' "$CARGO_VOL" > "$SMOKE/volume-cargo.txt"
printf '%s\n' "$TARGET_VOL" > "$SMOKE/volume-target.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$SMOKE/docker-run.exit"
  exit 2
fi

cat > "$SMOKE/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a241
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
phase=preoverlay
memory=6g
cpus=1
global_cap=10
archive=${ROOT}/archive
fixed=${FIXED}
live_env=unset
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
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${SMOKE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a241 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a241 \
  --label oo.task=column-acl-cli-review-smoke \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$SMOKE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$SMOKE/docker-run.exit"
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
} > "$SMOKE/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$SMOKE/mounts.txt"
docker rm "$NAME" > "$SMOKE/container-removed.txt"
exit "$run_exit"
