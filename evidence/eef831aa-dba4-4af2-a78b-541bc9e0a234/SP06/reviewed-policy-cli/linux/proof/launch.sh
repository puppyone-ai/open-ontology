#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Fixed archive baf70c5c plus three owned files. No PostgreSQL.
# Origin: a234 linux/smoke/launch.sh, itself from a227 smoke launch.sh ca7194b8f877386fff3339f0614ea03436b8a65b.
# Changes: copy the CLI bin, test, and contract onto the archive. 6GiB, 1 CPU.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a234/SP06/reviewed-policy-cli
PROOF=${EVIDENCE}/linux/proof
ROOT=/tmp/oo-a234-eef831aa-dba4-4af2-a78b-541bc9e0a234
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=baf70c5c7b6d01183a178f53c77f65da4a7bb497
NAME=oo-a234-proof
NET=oo-a234-eef831aa-net
CARGO_VOL=oo-a234-eef831aa-cargo
TARGET_VOL=oo-a234-eef831aa-target

chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh"
mkdir -p "$PROOF/phases" "$EVIDENCE/source/product"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
for path in \
  crates/oo-gateway/src/bin/reviewed-policy.rs \
  crates/oo-gateway/tests/reviewed_policy_cli.rs \
  contracts/gateway/reviewed-policy-cli-v1.md
do
  if [ ! -f "$SRC/$path" ]; then
    echo "missing workspace file $path" >&2
    printf '%s\n' 2 > "$PROOF/docker-run.exit"
    exit 2
  fi
done
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi

git -C "$SRC" rev-parse "$FIXED" > "$PROOF/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$PROOF/archive-tree.txt"
mkdir -p "$ROOT/archive"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates contracts \
  | tar -x -C "$ROOT/archive"
cp "$SRC/crates/oo-gateway/src/bin/reviewed-policy.rs" "$ROOT/archive/crates/oo-gateway/src/bin/reviewed-policy.rs"
cp "$SRC/crates/oo-gateway/tests/reviewed_policy_cli.rs" "$ROOT/archive/crates/oo-gateway/tests/reviewed_policy_cli.rs"
cp "$SRC/contracts/gateway/reviewed-policy-cli-v1.md" "$ROOT/archive/contracts/gateway/reviewed-policy-cli-v1.md"
mv "$ROOT/archive/rust-toolchain.toml" "$PROOF/rust-toolchain.toml.aside"
printf '%s\n' "moved aside; pinned image stays 1.94.0; git object unchanged" > "$PROOF/toolchain-aside.txt"
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
  done <<'BLOBS'
crates/oo-gateway/src/reviewed_policy.rs|reviewed_policy.rs
crates/oo-gateway/src/lib.rs|lib.rs
crates/oo-gateway/src/policy.rs|policy.rs
crates/oo-gateway/Cargo.toml|oo-gateway-Cargo.toml
Cargo.lock|Cargo.lock
BLOBS
} > "$PROOF/archive-blobs.txt"
if [ "$blob_ok" -ne 0 ]; then
  echo "archive blob mismatch" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
{
  for path in \
    crates/oo-gateway/src/bin/reviewed-policy.rs \
    crates/oo-gateway/tests/reviewed_policy_cli.rs \
    contracts/gateway/reviewed-policy-cli-v1.md
  do
    work=$(git -C "$SRC" hash-object "$SRC/$path")
    arch=$(git -C "$SRC" hash-object "$ROOT/archive/$path")
    printf '%s work=%s archive=%s\n' "$path" "$work" "$arch"
    if [ "$work" != "$arch" ]; then
      echo "overlay mismatch $path" >&2
      blob_ok=1
    fi
  done
} > "$PROOF/worktree-blobs.txt"
if [ "$blob_ok" -ne 0 ]; then
  echo "overlay blob mismatch" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
cp "$SRC/crates/oo-gateway/src/bin/reviewed-policy.rs" "$EVIDENCE/source/product/reviewed-policy.rs"
cp "$SRC/crates/oo-gateway/tests/reviewed_policy_cli.rs" "$EVIDENCE/source/product/reviewed_policy_cli.rs"
cp "$SRC/contracts/gateway/reviewed-policy-cli-v1.md" "$EVIDENCE/source/product/reviewed-policy-cli-v1.md"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a234 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a234 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$PROOF/network.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$PROOF/host-cargo-procs.txt" || true
host_n=$(grep -c . "$PROOF/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$PROOF/builders-before.txt" || true
builder_n=$(grep -c . "$PROOF/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$PROOF/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a234 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$PROOF/image.txt"
grep -q 'arch=arm64' "$PROOF/image.txt"
printf '%s\n' "$CARGO_VOL" > "$PROOF/volume-cargo.txt"
printf '%s\n' "$TARGET_VOL" > "$PROOF/volume-target.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi

cat > "$PROOF/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a234
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
phase=proof
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
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a234 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a234 \
  --label oo.task=reviewed-policy-cli-proof \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROOF/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROOF/docker-run.exit"
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
} > "$PROOF/hostconfig.txt"
docker inspect -f '{{json .Mounts}}' "$NAME" > "$PROOF/mounts.txt"
docker rm "$NAME" > "$PROOF/container-removed.txt"
exit "$run_exit"
