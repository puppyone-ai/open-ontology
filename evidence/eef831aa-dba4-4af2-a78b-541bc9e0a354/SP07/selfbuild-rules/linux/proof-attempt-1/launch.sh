#!/bin/bash
# One Linux arm64 Rust 1.94 builder. Focused proof. No PostgreSQL.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a354/SP07/selfbuild-rules/linux/smoke/launch.sh
# which follows evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/linux/smoke/launch.sh
# Change: mounts the owned crate read-only, root lock, three sealed JSON files, pids 4096, 6 GiB, 1 CPU.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a354/SP07/selfbuild-rules
PROOF=${EVIDENCE}/linux/proof
CRATE=${SRC}/tools/selfbuild-rules
SEALED=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/live/pass2
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=7a6f7eac8b8041eae215c087d1a80930437871ce
NAME=oo-a354-rules-proof
NET=oo-a354-eef831aa-net
CARGO_VOL=oo-a354-eef831aa-cargo
TARGET_VOL=oo-a354-eef831aa-target

chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh"
mkdir -p "$PROOF/phases"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi

git -C "$SRC" rev-parse "$FIXED" > "$PROOF/fixed-commit.txt"
{
  printf 'a354-smoke %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a354/SP07/selfbuild-rules/linux/smoke/launch.sh)"
  printf 'a328 %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/linux/smoke/launch.sh)"
  printf 'a200 %s\n' "$(git -C "$SRC" hash-object evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh)"
} > "$PROOF/recipe-origins.txt"

docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a354 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a354 \
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
own=$(docker ps -aq --filter status=running --filter label=oo.owner=a354 --filter label=oo.kind=rust-builder)
if [ -n "$own" ]; then
  echo "deferred: this task already has a rust builder" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$PROOF/image.txt"
grep -q 'arch=arm64' "$PROOF/image.txt"
docker volume inspect "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi

cat > "$PROOF/invocation.txt" <<EOF
run_id=eef831aa-dba4-4af2-a78b-541bc9e0a354
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
phase=proof
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
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SRC}/Cargo.lock,target=/locks/Cargo.lock,readonly" \
  --mount "type=bind,source=${SEALED}/catalog.json,target=/sealed/catalog.json,readonly" \
  --mount "type=bind,source=${SEALED}/profile.json,target=/sealed/profile.json,readonly" \
  --mount "type=bind,source=${SEALED}/cards.json,target=/sealed/cards.json,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a354 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a354 \
  --label oo.task=selfbuild-rules-proof \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROOF/console.log" 2>&1
run_status=$?
set -e
printf '%s\n' "$run_status" > "$PROOF/docker-run.exit"
docker inspect --format 'oom={{.State.OOMKilled}} status={{.State.Status}} exit={{.State.ExitCode}} mem={{.HostConfig.Memory}} cpu={{.HostConfig.NanoCpus}} priv={{.HostConfig.Privileged}} net={{.HostConfig.NetworkMode}} pidslimit={{.HostConfig.PidsLimit}}' "$NAME" > "$PROOF/hostconfig.txt"
docker inspect --format '{{json .Mounts}}' "$NAME" > "$PROOF/mounts.txt"
docker inspect --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' "$NAME" > "$PROOF/caps.txt"
docker rm "$NAME" >/dev/null
printf '%s\n' removed > "$PROOF/container-removed.txt"
exit "$run_status"
