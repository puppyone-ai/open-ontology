#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Immutable partial archive of ed661809. No PostgreSQL.
# Origin: a218 smoke launch.sh fc6bbb9d4a0759335561a38429e2d9426d652a3b
# and a217 partial archive launch.sh d715ba6bc1b0a7cd0f414abe05bd86c998319a49.
# Changes: 6GiB, 1 CPU, global cap 10, a227 volumes, gateway libclang, no PostgreSQL.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a227/SP06/reviewed-policy
RED=${EVIDENCE}/linux/red
ROOT=/tmp/oo-a227-eef831aa-dba4-4af2-a78b-541bc9e0a227
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=ed661809cf6c1e8df37af5842838dd1eb839c98a
NAME=oo-a227-red
NET=oo-a227-eef831aa-net
CARGO_VOL=oo-a227-eef831aa-cargo
TARGET_VOL=oo-a227-eef831aa-target

chmod 755 "$RED/inside.sh" "$RED/launch.sh" "$RED/extract-libclang.sh"
mkdir -p "$RED/phases" "$EVIDENCE/source/blobs"
if [ -f "$RED/console.log" ]; then
  echo "refusing to overwrite smoke console.log" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$RED/docker-run.exit"
  exit 1
fi

git -C "$SRC" rev-parse "$FIXED" > "$RED/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$RED/archive-tree.txt"
mkdir -p "$ROOT"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/policy.rs" ]; then
  echo "archive missing policy.rs" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/crates/oo-gateway/src/reviewed_policy.rs" ]; then
  echo "archive already contains reviewed_policy.rs" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/contracts/gateway/reviewed-policy-v1.md" ]; then
  echo "archive already contains the reviewed-policy contract" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi
git -C "$SRC" show "$FIXED:rust-toolchain.toml" > "$RED/rust-toolchain.toml.fixed"
mv "$ROOT/archive/rust-toolchain.toml" "$RED/rust-toolchain.toml.aside"
printf '%s\n' "moved aside; pinned image stays 1.94.0; git object unchanged" > "$RED/toolchain-aside.txt"
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
crates/oo-gateway/src/policy.rs|policy.rs
crates/oo-gateway/src/lib.rs|lib.rs
crates/oo-gateway/Cargo.toml|oo-gateway-Cargo.toml
Cargo.lock|Cargo.lock
BLOBS
} > "$RED/archive-blobs.txt"
if [ "$blob_ok" -ne 0 ]; then
  echo "archive blob mismatch" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi
printf '%s\n' "$FIXED" > "$EVIDENCE/source/COMMIT"
rm -f "$ROOT/archive/crates/oo-gateway/src/reviewed_policy.rs"
cp "$SRC/crates/oo-gateway/tests/reviewed_policy.rs" \
  "$ROOT/archive/crates/oo-gateway/tests/reviewed_policy.rs"
if [ -f "$ROOT/archive/crates/oo-gateway/src/reviewed_policy.rs" ]; then
  echo "red archive must not contain the adapter implementation" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi
printf '%s\n' "implementation=absent test=present archive=$FIXED" > "$RED/overlay-absent.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a227 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a227 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$RED/network.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$RED/host-cargo-procs.txt" || true
host_n=$(grep -c . "$RED/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$RED/builders-before.txt" || true
builder_n=$(grep -c . "$RED/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$RED/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$RED/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a227 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$RED/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$RED/image.txt"
grep -q 'arch=arm64' "$RED/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a227 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a227 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a227 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a227 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
printf '%s\n' "$CARGO_VOL" > "$RED/volume-cargo.txt"
printf '%s\n' "$TARGET_VOL" > "$RED/volume-target.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named smoke container already exists" >&2
  printf '%s\n' 2 > "$RED/docker-run.exit"
  exit 2
fi

cat > "$RED/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a227
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
phase=red
memory=6g
cpus=1
global_cap=10
archive=${ROOT}/archive
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
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${RED}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${RED}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a227 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a227 \
  --label oo.task=reviewed-policy-red \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$RED/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$RED/docker-run.exit"
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
} > "$RED/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$RED/mounts.txt"
docker rm "$NAME" > "$RED/container-removed.txt"
exit "$run_exit"
