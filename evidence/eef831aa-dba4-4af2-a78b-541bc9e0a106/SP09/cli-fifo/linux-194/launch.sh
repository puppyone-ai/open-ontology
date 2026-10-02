#!/bin/bash
# Fixed oo-cli Linux arm64 Rust 1.94 tests and Clippy.
# Archive of the pre-fix commit plus the owned CLI overlay only.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
EVID="$ROOT/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a106/SP09/cli-fifo/linux-194"
COMMIT="dde0dff30e64c9f4c30a7b1371508de38efea553"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
UUID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
WORK="/tmp/oo-a106-$UUID"
SNAP="$WORK/snapshot"
OUT="$EVID/out"
NET="oo-a106-$UUID-net"
CNAME="oo-a106-msrv-$UUID"
VOL_CARGO="oo-a106-$UUID-cargo"
VOL_TARGET="oo-a106-$UUID-target"

mkdir -p "$OUT/phases" "$SNAP"
printf '%s\n' "$UUID" > "$EVID/run-id.txt"
printf '%s\n' "$WORK" > "$EVID/work-path.txt"
printf '%s\n' "$CNAME" > "$EVID/container-name.txt"

{
  printf 'run_id=%s\n' "$UUID"
  printf 'archive_commit=%s\n' "$COMMIT"
  printf 'parent=%s\n' "$(git -C "$ROOT" rev-parse "${COMMIT}^")"
  printf 'tree=%s\n' "$(git -C "$ROOT" rev-parse "${COMMIT}^{tree}")"
  printf 'image=%s\n' "$IMAGE"
  printf 'platform=linux/arm64\n'
  printf 'toolchain=1.94.0\n'
  printf 'network=%s\n' "$NET"
  printf 'container=%s\n' "$CNAME"
  printf 'cargo_volume=%s\n' "$VOL_CARGO"
  printf 'target_volume=%s\n' "$VOL_TARGET"
  printf 'snapshot_method=git archive plus owned oo-cli contracts/cli and Cargo.lock overlay\n'
  printf 'privileged=no\n'
  printf 'network_mode=bridge_not_host\n'
  printf 'home_mount=no\n'
  printf 'socket_mount=no\n'
  printf 'shared_cargo_cache=no\n'
  printf 'pull=never\n'
  printf 'cap_drop=ALL\n'
  printf 'cap_add=none\n'
  printf 'init=yes\n'
  printf 'no_new_privileges=yes\n'
  printf 'memory=8g\n'
  printf 'cpus=4\n'
  printf 'pids_limit=4096\n'
  printf 'fetch=cargo fetch --manifest-path Cargo.toml --locked\n'
  printf 'test=cargo test --manifest-path Cargo.toml --locked --offline -p oo-cli\n'
  printf 'clippy=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-cli --all-targets -- -D warnings\n'
} > "$EVID/invocation.txt"

{
  echo "host_uname=$(uname -m)"
  echo "running_containers_before:"
  docker ps --format 'name={{.Names}} status={{.Status}} owner={{.Label "oo.owner"}} kind={{.Label "oo.kind"}}'
  echo "host_toolchain_comms:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk 'tolower($4) ~ /^(cargo|rustc|rustdoc|rustup)$/ { print }'
  echo "note=capacity counted by running container label and comm only"
  echo "a108_volumes_not_used=oo-a108-eef831aa-dba4-4af2-a78b-541bc9e0a108-cargo oo-a108-eef831aa-dba4-4af2-a78b-541bc9e0a108-target"
} > "$EVID/capacity.txt"

builders=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | wc -l | tr -d ' ')
own=$(docker ps --filter label=oo.owner=a106 --filter label=oo.kind=rust-builder --format '{{.Names}}' | wc -l | tr -d ' ')
host_n=$(ps -ax -o comm= | awk 'tolower($0) ~ /^(cargo|rustc|rustdoc|rustup)$/' | wc -l | tr -d ' ')
global=$((builders + host_n))
{
  echo "labeled_rust_builders=$builders"
  echo "own_rust_builders=$own"
  echo "host_toolchain_comms=$host_n"
  echo "global=$global"
} >> "$EVID/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 2 ]; then
  echo "capacity_full_before_start" >> "$EVID/capacity.txt"
  printf 'not_started\n' > "$EVID/docker-run.exit"
  exit 2
fi

git -C "$ROOT" archive --format=tar -o "$WORK/delivery.tar" "$COMMIT"
tar -xf "$WORK/delivery.tar" -C "$SNAP"
rm -rf "$SNAP/crates/oo-cli" "$SNAP/contracts/cli"
mkdir -p "$SNAP/contracts"
cp -R "$ROOT/crates/oo-cli" "$SNAP/crates/oo-cli"
cp -R "$ROOT/contracts/cli" "$SNAP/contracts/cli"
cp "$ROOT/Cargo.lock" "$SNAP/Cargo.lock"
rm -rf "$SNAP/crates/oo-cli/target"

shasum -a 256 "$WORK/delivery.tar" > "$EVID/archive.tar.sha256"
{
  echo "archive_commit=$COMMIT"
  echo "archive_tree=$(git -C "$ROOT" rev-parse "${COMMIT}^{tree}")"
  echo "archive_parent=$(git -C "$ROOT" rev-parse "${COMMIT}^")"
  echo "archive_lock_blob=$(git -C "$ROOT" rev-parse "${COMMIT}:Cargo.lock")"
  echo "overlay_lock_blob=$(git -C "$ROOT" hash-object "$SNAP/Cargo.lock")"
  echo "overlay_lock_sha256=$(shasum -a 256 "$SNAP/Cargo.lock" | awk '{print $1}')"
  echo "lock_matches_archive=$([ "$(git -C "$ROOT" hash-object "$SNAP/Cargo.lock")" = "$(git -C "$ROOT" rev-parse "${COMMIT}:Cargo.lock")" ] && echo yes || echo no)"
} > "$EVID/input-refs.txt"
(
  cd "$SNAP"
  find crates/oo-cli contracts/cli Cargo.lock -type f -print0 | sort -z | xargs -0 shasum -a 256
) > "$EVID/overlay-files.sha256"
shasum -a 256 "$EVID/inside.sh" > "$EVID/inside.sha256"

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVID/image.txt"
if ! docker network create --label oo.owner=a106 --label oo.run="$UUID" --label oo.kind=rust-builder "$NET" > "$EVID/network-create.txt"; then
  printf 'network_create_failed\n' > "$EVID/docker-run.exit"
  exit 1
fi
if ! docker volume create --label oo.owner=a106 --label oo.run="$UUID" --label oo.kind=rust-builder "$VOL_CARGO" > "$EVID/volume-cargo.txt"; then
  printf 'volume_cargo_failed\n' > "$EVID/docker-run.exit"
  docker network rm "$NET" >/dev/null 2>&1 || true
  exit 1
fi
if ! docker volume create --label oo.owner=a106 --label oo.run="$UUID" --label oo.kind=rust-builder "$VOL_TARGET" > "$EVID/volume-target.txt"; then
  printf 'volume_target_failed\n' > "$EVID/docker-run.exit"
  docker volume rm "$VOL_CARGO" >/dev/null 2>&1 || true
  docker network rm "$NET" >/dev/null 2>&1 || true
  exit 1
fi

set +e
docker run \
  --name "$CNAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --label oo.owner=a106 \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVID}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${OUT},target=/out" \
  --mount "type=volume,source=${VOL_CARGO},target=/cargo" \
  --mount "type=volume,source=${VOL_TARGET},target=/target" \
  "$IMAGE" \
  bash /inside.sh
run_ec=$?
printf '%s\n' "$run_ec" > "$EVID/docker-run.exit"
docker inspect "$CNAME" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} CapAdd={{json .HostConfig.CapAdd}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}}' > "$EVID/inspect.txt"
docker inspect "$CNAME" --format '{{json .HostConfig.Tmpfs}}' > "$EVID/tmpfs.txt"
docker inspect "$CNAME" --format '{{range .Mounts}}{{.Type}} dst={{.Destination}} rw={{.RW}} name={{.Name}}{{println}}{{end}}' > "$EVID/mounts.txt"

{
  echo "container_rm=$(docker rm -f "$CNAME" >/dev/null && echo 0 || echo $?)"
  echo "network_rm=$(docker network rm "$NET" >/dev/null && echo 0 || echo $?)"
  echo "volume_cargo_rm=$(docker volume rm "$VOL_CARGO" >/dev/null && echo 0 || echo $?)"
  echo "volume_target_rm=$(docker volume rm "$VOL_TARGET" >/dev/null && echo 0 || echo $?)"
} > "$EVID/cleanup.txt"
rm -rf "$WORK"
printf 'work_removed=yes\n' >> "$EVID/cleanup.txt"
echo "remaining_a106:" >> "$EVID/cleanup.txt"
docker ps -a --filter label=oo.owner=a106 --format 'name={{.Names}} status={{.Status}}' >> "$EVID/cleanup.txt"
docker volume ls --filter label=oo.owner=a106 --format '{{.Name}}' >> "$EVID/cleanup.txt"
docker network ls --filter label=oo.owner=a106 --format '{{.Name}}' >> "$EVID/cleanup.txt"
exit "$run_ec"
