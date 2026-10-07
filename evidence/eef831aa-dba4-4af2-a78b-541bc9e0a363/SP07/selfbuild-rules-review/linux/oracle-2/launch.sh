#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Oracle-only retry. No PostgreSQL.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a363/SP07/selfbuild-rules-review/linux/proof/launch.sh
# Change: compile and run the hand oracle only. Reuse the private volumes. Do not rerun passed campaigns.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a363/SP07/selfbuild-rules-review
HERE=${EVIDENCE}/linux/oracle-2
PROOF=${EVIDENCE}/linux/proof
CRATE=${SRC}/tools/selfbuild-rules
SEALED=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/live/pass2
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=e5427c1a18e2ddbb480791d214c76888eb7ca998
NAME=oo-a363-rules-oracle2
NET=oo-a363-eef831aa-net
CARGO_VOL=oo-a363-eef831aa-cargo
TARGET_VOL=oo-a363-eef831aa-target

chmod 755 "$HERE/inside.sh" "$HERE/launch.sh"
mkdir -p "$HERE/phases"
if [ -f "$HERE/console.log" ]; then
  echo "refusing to overwrite oracle-2 console.log" >&2
  printf '%s\n' 2 > "$HERE/docker-run.exit"
  exit 2
fi
if [ ! -f "$PROOF/console.log" ] || [ ! -f "$PROOF/shop-review.json" ]; then
  echo "proof artifacts are missing; refusing to regenerate them here" >&2
  printf '%s\n' 2 > "$HERE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$HERE/docker-run.exit"
  exit 1
fi

git -C "$SRC" rev-parse "$FIXED" > "$HERE/fixed-commit.txt"
{
  printf 'a363-proof-launch %s\n' "$(git -C "$SRC" hash-object "$PROOF/launch.sh" 2>/dev/null || true)"
  printf 'a354-proof %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a354/SP07/selfbuild-rules/linux/proof/launch.sh)"
  printf 'a354-proof-inside %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a354/SP07/selfbuild-rules/linux/proof/inside.sh)"
} > "$HERE/recipe-origins.txt"

docker network inspect "$NET" >/dev/null
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
docker network inspect --format '{{.Id}} {{.Driver}} internal={{.Internal}}' "$NET" > "$HERE/network.txt"

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$HERE/host-cargo-procs.txt" || true
host_n=$(grep -c . "$HERE/host-cargo-procs.txt" || true)
{
  docker ps --filter status=running --filter label=oo.kind=rust-builder --format '{{.Names}}'
  docker ps --filter status=running --filter ancestor=sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5 --format '{{.Names}}'
} | awk 'NF && !seen[$0]++' > "$HERE/builders-before.txt" || true
builder_n=$(grep -c . "$HERE/builders-before.txt" || true)
global_n=$((host_n + builder_n))
printf 'host_cargo=%s rust_builders=%s global=%s cap=10\n' "$host_n" "$builder_n" "$global_n" > "$HERE/capacity.txt"
if [ "$global_n" -ge 10 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  printf '%s\n' 3 > "$HERE/docker-run.exit"
  exit 3
fi
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a363 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$HERE/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$HERE/image.txt"
grep -q 'arch=arm64' "$HERE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named oracle container already exists" >&2
  printf '%s\n' 2 > "$HERE/docker-run.exit"
  exit 2
fi

cat > "$HERE/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a363
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
phase=oracle-2
skipped=smoke,fetch,test,clippy,lock-subset,sealed-cli
global_cap=10
memory=6g
cpus=1
pids=4096
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
  -e SEALED_CATALOG=/sealed/catalog.json \
  -e SEALED_PROFILE=/sealed/profile.json \
  -e SEALED_CARDS=/sealed/cards.json \
  --mount "type=bind,source=${CRATE},target=/work,readonly" \
  --mount "type=bind,source=${HERE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEALED}/catalog.json,target=/sealed/catalog.json,readonly" \
  --mount "type=bind,source=${SEALED}/profile.json,target=/sealed/profile.json,readonly" \
  --mount "type=bind,source=${SEALED}/cards.json,target=/sealed/cards.json,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a363 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a363 \
  --label oo.task=selfbuild-rules-review-oracle-2 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$HERE/console.log" 2>&1
run_status=$?
set -e
printf '%s\n' "$run_status" > "$HERE/docker-run.exit"
docker inspect --format 'oom={{.State.OOMKilled}} status={{.State.Status}} exit={{.State.ExitCode}} mem={{.HostConfig.Memory}} cpu={{.HostConfig.NanoCpus}} priv={{.HostConfig.Privileged}} net={{.HostConfig.NetworkMode}} pidslimit={{.HostConfig.PidsLimit}}' "$NAME" > "$HERE/hostconfig.txt"
docker inspect --format '{{json .Mounts}}' "$NAME" > "$HERE/mounts.txt"
docker inspect --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' "$NAME" > "$HERE/caps.txt"
docker rm "$NAME" >/dev/null
printf '%s\n' removed > "$HERE/container-removed.txt"
exit "$run_status"
