#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Post-edit proof of fixed c23db28 plus the owned overlay.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a263/SP07/description-provider/linux/proof/inside.sh
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/linux/focused-6/launch.sh
# Change: fixed commit c23db28, workspace overlay of tools/pg-builder and testing/real-db/pg-builder, lock copy-back, 6GiB, 1 CPU.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder
DIR=${EVIDENCE}/linux/focused-8
ROOT=/tmp/oo-a328-eef831aa-dba4-4af2-a78b-541bc9e0a328/focused-8
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=c23db28dc27f5cf20741dd36e8a8e88129ad1715
NAME=oo-a328-focused-8
NET=oo-a328-eef831aa-net
CARGO_VOL=oo-a328-eef831aa-cargo
TARGET_VOL=oo-a328-eef831aa-target

chmod 755 "$DIR/inside.sh" "$DIR/launch.sh"
mkdir -p "$DIR/phases" "$DIR/locks"
if [ -f "$DIR/console.log" ]; then
  echo "refusing to overwrite focused-8 console.log" >&2
  printf '%s\n' 2 > "$DIR/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$DIR/docker-run.exit"
  exit 1
fi
git -C "$SRC" rev-parse "$FIXED" > "$DIR/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$DIR/archive-tree.txt"
git -C "$SRC" rev-parse HEAD > "$DIR/repo-head.txt"
git -C "$SRC" ls-tree -r --name-only "$FIXED" -- tools/pg-builder testing/real-db/pg-builder > "$DIR/owned-in-archive.txt"
if ! grep -q '^tools/pg-builder/src/lib.rs$' "$DIR/owned-in-archive.txt"; then
  echo "fixed commit is missing the owned builder" >&2
  printf '%s\n' 2 > "$DIR/docker-run.exit"
  exit 2
fi
{
  printf 'a200 %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/launch.sh)"
  printf 'a204 %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh)"
  printf 'a199 %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/manifest/launch.sh)"
  printf 'a263 %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a263/SP07/description-provider/linux/proof/inside.sh)"
  printf 'a328-focused-6 %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/linux/focused-6/launch.sh)"
} > "$DIR/recipe-origins.txt"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  tools/description-pack tools/description-review \
  tools/pg-builder testing/real-db/pg-builder \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/Cargo.lock" ] || [ ! -f "$ROOT/archive/tools/pg-builder/src/lib.rs" ]; then
  echo "fixed archive is missing the compiler closure or the builder" >&2
  printf '%s\n' 2 > "$DIR/docker-run.exit"
  exit 2
fi
rm -rf "$ROOT/archive/tools/pg-builder" "$ROOT/archive/testing/real-db/pg-builder"
mkdir -p "$ROOT/archive/tools" "$ROOT/archive/testing/real-db"
cp -a "$SRC/tools/pg-builder" "$ROOT/archive/tools/pg-builder"
cp -a "$SRC/testing/real-db/pg-builder" "$ROOT/archive/testing/real-db/pg-builder"
rm -rf "$ROOT/archive/tools/pg-builder/target" "$ROOT/archive/testing/real-db/pg-builder/target"
rm -f "$ROOT/archive/tools/pg-builder/Cargo.lock" "$ROOT/archive/testing/real-db/pg-builder/Cargo.lock"
find "$ROOT/archive/tools/pg-builder" "$ROOT/archive/testing/real-db/pg-builder" -name target -type d -prune -print > "$DIR/overlay-target-dirs.txt"
printf 'overlay=workspace\nbase=%s\n' "$FIXED" > "$DIR/preedit.txt"

PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
visible=0
attempt=0
for attempt in 1 2 3 4 5 6 7 8 9 10 11 12; do
  if docker run --rm --pull never --network none --entrypoint bash \
    --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
    "$PG_IMAGE" -c 'test -f /src/tools/pg-builder/src/lib.rs' >/dev/null 2>&1; then
    visible=1
    break
  fi
  sleep 1
done
printf 'visible=%s attempts=%s\n' "$visible" "$attempt" > "$DIR/archive-visible.txt"
if [ "$visible" != 1 ]; then
  echo "archive is not visible to docker" >&2
  printf '%s\n' 125 > "$DIR/docker-run.exit"
  exit 125
fi

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a328 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a328 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$DIR/network.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$DIR/host-cargo-procs.txt" || true
host_n=$(grep -c . "$DIR/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$DIR/builders-before.txt" || true
builder_n=$(grep -c . "$DIR/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$DIR/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$DIR/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a328 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$DIR/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$DIR/image.txt"
grep -q 'arch=arm64' "$DIR/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a328 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a328 \
  --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a328 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a328 \
  --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named focused container already exists" >&2
  printf '%s\n' 2 > "$DIR/docker-run.exit"
  exit 2
fi

cat > "$DIR/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a328
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
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
phase=focused-postedit
global_cap=10
memory=6g
cpus=1
fixed=${FIXED}
overlay=workspace
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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target/pg-builder \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${DIR}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a328 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a328 \
  --label oo.task=pg-builder-focused-8 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$DIR/console.log" 2>&1
run_status=$?
set -e
printf '%s\n' "$run_status" > "$DIR/docker-run.exit"
docker inspect --format 'oom={{.State.OOMKilled}} status={{.State.Status}} exit={{.State.ExitCode}} mem={{.HostConfig.Memory}} cpu={{.HostConfig.NanoCpus}} priv={{.HostConfig.Privileged}} net={{.HostConfig.NetworkMode}} pidslimit={{.HostConfig.PidsLimit}}' "$NAME" > "$DIR/hostconfig.txt"
docker inspect --format '{{json .HostConfig.Binds}}' "$NAME" > "$DIR/mounts.txt"
docker inspect --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' "$NAME" > "$DIR/caps.txt"
docker rm "$NAME" >/dev/null
printf '%s\n' removed > "$DIR/container-removed.txt"
if [ -f "$DIR/locks/pg-builder-Cargo.lock" ] && [ -f "$DIR/phases/lock-subset-builder.exit" ] && [ "$(cat "$DIR/phases/lock-subset-builder.exit")" = "0" ]; then
  cp "$DIR/locks/pg-builder-Cargo.lock" "$SRC/tools/pg-builder/Cargo.lock"
fi
if [ -f "$DIR/locks/pg-fixture-Cargo.lock" ] && [ -f "$DIR/phases/lock-subset-fixture.exit" ] && [ "$(cat "$DIR/phases/lock-subset-fixture.exit")" = "0" ]; then
  cp "$DIR/locks/pg-fixture-Cargo.lock" "$SRC/testing/real-db/pg-builder/Cargo.lock"
fi
exit "$run_status"
