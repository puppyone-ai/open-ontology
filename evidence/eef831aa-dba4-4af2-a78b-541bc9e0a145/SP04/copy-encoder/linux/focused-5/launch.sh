#!/bin/bash
# Fixed archive plus the four owned copy-encoder paths. One rust builder, one private PG 17.10.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a145/SP04/copy-encoder/linux/focused-5
ROOT=/tmp/oo-a145-eef831aa-dba4-4af2-a78b-541bc9e0a145
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NAME=oo-a145-focused-5
PG=oo-a145-pg
NET=oo-a145-eef831aa-net
CARGO_VOL=oo-a145-eef831aa-cargo
TARGET_VOL=oo-a145-eef831aa-target
PG_VOL=oo-a145-eef831aa-pg5
FIXED=6b95d6f2fd2259edfcbffc27a6338518a64dd0fc
EXPECT_TREE=baaabaf1384646237e0c40bfa1914ea150d39c5b
SNAP=${ROOT}/focused-5-src
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a145
created_pg=0
success=0

mkdir -p "$EVIDENCE" "$ROOT"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite focused-5 console.log" >&2
  exit 2
fi
chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"

cleanup() {
  docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' > "$EVIDENCE/inspect.txt" 2>/dev/null || true
  docker inspect "$NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} readonly_root={{.HostConfig.ReadonlyRootfs}} memory={{.HostConfig.Memory}} memoryswap={{.HostConfig.MemorySwap}}' > "$EVIDENCE/hostconfig.txt" 2>/dev/null || true
  docker inspect "$NAME" --format '{{json .HostConfig.Binds}}{{json .HostConfig.Mounts}}' > "$EVIDENCE/mounts.txt" 2>/dev/null || true
  docker inspect "$PG" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} memory={{.HostConfig.Memory}}' > "$EVIDENCE/postgres-hostconfig.txt" 2>/dev/null || true
  docker logs "$PG" > "$EVIDENCE/postgres.log" 2>&1 || true
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker rm -f "$PG" >/dev/null 2>&1 || true
  if [ "$success" -eq 1 ]; then
    docker volume rm "$CARGO_VOL" > "$EVIDENCE/cleanup-cargo.txt" 2>&1 || echo "cargo volume rm failed" >> "$EVIDENCE/cleanup-failures.txt"
    docker volume rm "$TARGET_VOL" > "$EVIDENCE/cleanup-target.txt" 2>&1 || echo "target volume rm failed" >> "$EVIDENCE/cleanup-failures.txt"
    docker volume rm "$PG_VOL" > "$EVIDENCE/cleanup-pg.txt" 2>&1 || echo "pg volume rm failed" >> "$EVIDENCE/cleanup-failures.txt"
    docker network rm "$NET" > "$EVIDENCE/cleanup-net.txt" 2>&1 || echo "network rm failed" >> "$EVIDENCE/cleanup-failures.txt"
  elif [ "$created_pg" -eq 1 ]; then
    docker volume rm "$PG_VOL" > "$EVIDENCE/cleanup-pg.txt" 2>&1 || echo "pg volume rm failed" >> "$EVIDENCE/cleanup-failures.txt"
  fi
}
trap cleanup EXIT

head_now=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$head_now" > "$EVIDENCE/workspace-head.txt"

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "private network from the smoke run is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}'
  echo "host_toolchain_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver|rustup)$/ {print}' || true
} > "$EVIDENCE/builders-before.txt"

running_rust=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
own=$(docker ps -q --filter label=oo.owner=a145 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
printf 'running_rust_builders=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi

actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
printf '%s\n' "$FIXED" > "$EVIDENCE/archive-base.txt"
printf '%s\n' "$actual_tree" > "$EVIDENCE/archive-tree.txt"
test "$actual_tree" = "$EXPECT_TREE"
rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
overlay=(
  crates/oo-pipeline/src/lib.rs
  crates/oo-pipeline/src/copy_encoder.rs
  crates/oo-pipeline/tests/copy_encoder_live.rs
  contracts/pipeline/copy-encoder-v1.md
)
for rel in "${overlay[@]}"; do
  cp "$SRC/$rel" "$SNAP/$rel"
done
head_after=$(git -C "$SRC" rev-parse HEAD)
test "$head_after" = "$head_now"

file_sha() {
  shasum -a 256 "$1" | awk '{print $1}'
}
fixed_blob() {
  git -C "$SRC" rev-parse "${FIXED}:${1}"
}
for rel in \
  crates/oo-pipeline/src/physical_mapping.rs \
  crates/oo-pipeline/Cargo.toml \
  contracts/source/pgsql/type-conversion-v1.md
do
  test "$(git -C "$SRC" hash-object "$SNAP/$rel")" = "$(fixed_blob "$rel")"
done
test "$(git -C "$SRC" hash-object "$SNAP/crates/oo-pipeline/src/lib.rs")" != "$(fixed_blob crates/oo-pipeline/src/lib.rs)"
for rel in "${overlay[@]}"; do
  test "$(git -C "$SRC" hash-object "$SNAP/$rel")" = "$(git -C "$SRC" hash-object "$SRC/$rel")"
done
file_sha "$SNAP/crates/oo-pipeline/src/lib.rs" > "$EVIDENCE/expect-lib.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/copy_encoder.rs" > "$EVIDENCE/expect-encoder.sha256"
file_sha "$SNAP/crates/oo-pipeline/tests/copy_encoder_live.rs" > "$EVIDENCE/expect-live.sha256"
file_sha "$SNAP/crates/oo-pipeline/src/physical_mapping.rs" > "$EVIDENCE/expect-mapping.sha256"
file_sha "$SNAP/crates/oo-pipeline/Cargo.toml" > "$EVIDENCE/expect-cargo.sha256"
file_sha "$SNAP/contracts/pipeline/copy-encoder-v1.md" > "$EVIDENCE/expect-contract.sha256"
file_sha "$SNAP/contracts/source/pgsql/type-conversion-v1.md" > "$EVIDENCE/expect-types.sha256"
{
  echo "lib_differs_from_fixed=1"
  echo "fixed_lib=$(fixed_blob crates/oo-pipeline/src/lib.rs)"
  echo "overlay_lib=$(git -C "$SRC" hash-object "$SNAP/crates/oo-pipeline/src/lib.rs")"
  echo "mapping_unchanged=1"
  echo "cargo_unchanged=1"
  echo "type_contract_unchanged=1"
} > "$EVIDENCE/overlay-notes.txt"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$RUST_IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$PG_IMAGE" > "$EVIDENCE/postgres-image.txt"
grep -q 'arch=arm64' "$EVIDENCE/postgres-image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
if docker volume inspect "$PG_VOL" >/dev/null 2>&1; then
  echo "refusing a pre-existing postgres volume" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if docker container inspect "$NAME" >/dev/null 2>&1 || docker container inspect "$PG" >/dev/null 2>&1; then
  echo "named focused container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology --label oo.owner=a145 \
  --label oo.session="$SESSION" --label oo.kind=postgres \
  "$PG_VOL" >/dev/null
created_pg=1

cat > "$EVIDENCE/invocation.txt" <<EOF
source_commit=${FIXED}
tree=${actual_tree}
workspace_head=${head_now}
composition=git archive of ${FIXED} plus overlay of lib.rs copy_encoder.rs copy_encoder_live.rs copy-encoder-v1.md
image=${RUST_IMAGE}
postgres_image=${PG_IMAGE}
platform=linux/arm64
toolchain=1.94.0
container=${NAME}
postgres=${PG}
network=${NET}
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
auth=trust
phases=rustfmt-component clippy-component rustfmt fetch test clippy live
package=oo-pipeline
EOF

docker run -d \
  --name "$PG" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER --cap-add SETGID --cap-add SETUID \
  --memory 1g --memory-swap 1g --cpus 1 --pids-limit 256 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,nodev,size=256m \
  --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  --label oo.project=open-ontology \
  --label oo.owner=a145 \
  --label oo.session="$SESSION" \
  --label oo.task=copy-encoder-focused-5 \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  "$PG_IMAGE" \
  -c listen_addresses='*' -c max_connections=40 -c shared_preload_libraries= \
  > "$EVIDENCE/postgres-start.txt"

ready=0
for _ in $(seq 1 60); do
  if docker exec "$PG" pg_isready -U postgres -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
printf 'ready=1\n' > "$EVIDENCE/postgres-ready.txt"

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
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e OO_COPY_ENCODER_HOST="$PG" \
  -e OO_COPY_ENCODER_PORT=5432 \
  -e OO_COPY_ENCODER_EVIDENCE=/evidence \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a145 \
  --label oo.session="$SESSION" \
  --label oo.task=copy-encoder-focused-5 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
if [ "$run_exit" -eq 0 ]; then
  success=1
fi
exit "$run_exit"
