#!/bin/bash
# a111 fixed-commit smoke, then one independent probe. Slot B only.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
EVID="$ROOT/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a111/SP09/cli-fifo-review"
COMMIT="7acca2073ad5f7d39d24a61a2e869a8d96fe1c6c"
PARENT="dde0dff30e64c9f4c30a7b1371508de38efea553"
IMAGE="docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
UUID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
WORK="/tmp/oo-a111-$UUID"
SNAP="$WORK/snapshot"
NET="oo-a111-$UUID-net"
CNAME="oo-a111-msrv-$UUID"
PNAME="oo-a111-probe-$UUID"
VOL_CARGO="oo-a111-$UUID-cargo"
VOL_TARGET="oo-a111-$UUID-target"
SMOKE="$EVID/linux-smoke"
PROBE="$EVID/probe-run/attempt-1"

mkdir -p "$SMOKE/out/phases" "$PROBE/out/phases" "$EVID/probe"
exec > >(tee -a "$EVID/launch.log") 2>&1

printf '%s\n' "$UUID" > "$EVID/run-id.txt"
printf '%s\n' "$WORK" > "$EVID/work-path.txt"
printf '%s\n' "$CNAME" > "$SMOKE/container-name.txt"
printf '%s\n' "$PNAME" > "$PROBE/container-name.txt"

capacity() {
  local label="$1"
  local dest="$2"
  {
    echo "when=$label"
    echo "host_uname=$(uname -m)"
    echo "running_containers:"
    docker ps --format 'name={{.Names}} status={{.Status}} owner={{.Label "oo.owner"}} kind={{.Label "oo.kind"}}'
    echo "host_toolchain_comms:"
    ps -ax -o pid=,ppid=,etime=,comm= | awk 'tolower($4) ~ /^(cargo|rustc|rustdoc|rustup)$/ { print }'
    echo "note=capacity counted by running rust-builder containers and cargo/rustc/rustdoc/rustup comm only"
    echo "postgres_not_counted=yes"
    echo "shell_text_not_counted=yes"
  } > "$dest"
  local builders own host_n global
  builders=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | wc -l | tr -d ' ')
  own=$(docker ps --filter label=oo.owner=a111 --filter label=oo.kind=rust-builder --format '{{.Names}}' | wc -l | tr -d ' ')
  host_n=$(ps -ax -o comm= | awk 'tolower($0) ~ /^(cargo|rustc|rustdoc|rustup)$/' | wc -l | tr -d ' ')
  global=$((builders + host_n))
  {
    echo "labeled_rust_builders=$builders"
    echo "own_rust_builders=$own"
    echo "host_toolchain_comms=$host_n"
    echo "global=$global"
  } >> "$dest"
  if [ "$own" -ge 1 ] || [ "$global" -ge 2 ]; then
    echo "capacity_full_before_start" >> "$dest"
    return 2
  fi
  return 0
}

cleanup() {
  local c_rm n_rm v1 v2
  c_rm=0
  if docker ps -a --format '{{.Names}}' | grep -qx "$CNAME"; then
    docker rm -f "$CNAME" >/dev/null 2>&1 || c_rm=$?
  fi
  if docker ps -a --format '{{.Names}}' | grep -qx "$PNAME"; then
    docker rm -f "$PNAME" >/dev/null 2>&1 || c_rm=$?
  fi
  if docker network ls --format '{{.Name}}' | grep -qx "$NET"; then
    n_rm=$(docker network rm "$NET" >/dev/null 2>&1 && echo 0 || echo 1)
  else
    n_rm=0
  fi
  if docker volume ls --format '{{.Name}}' | grep -qx "$VOL_CARGO"; then
    v1=$(docker volume rm "$VOL_CARGO" >/dev/null 2>&1 && echo 0 || echo 1)
  else
    v1=0
  fi
  if docker volume ls --format '{{.Name}}' | grep -qx "$VOL_TARGET"; then
    v2=$(docker volume rm "$VOL_TARGET" >/dev/null 2>&1 && echo 0 || echo 1)
  else
    v2=0
  fi
  if [ -d "$WORK" ]; then
    rm -rf "$WORK"
    work_removed=yes
  else
    work_removed=absent
  fi
  {
    echo "container_rm=$c_rm"
    echo "network_rm=$n_rm"
    echo "volume_cargo_rm=$v1"
    echo "volume_target_rm=$v2"
    echo "work_removed=$work_removed"
    echo "remaining_a111_containers:"
    docker ps -a --filter label=oo.owner=a111 --format '{{.Names}} {{.Label "oo.kind"}}' || true
    echo "remaining_a111_volumes:"
    docker volume ls --filter label=oo.owner=a111 --format '{{.Name}}' || true
    echo "remaining_a111_networks:"
    docker network ls --filter label=oo.owner=a111 --format '{{.Name}}' || true
  } > "$EVID/cleanup.txt"
}
trap cleanup EXIT

if ! capacity before_smoke "$EVID/capacity.txt"; then
  printf 'not_started\n' > "$SMOKE/docker-run.exit"
  exit 2
fi

mkdir -p "$SNAP"
git -C "$ROOT" archive --format=tar -o "$WORK/delivery.tar" "$COMMIT"
tar -xf "$WORK/delivery.tar" -C "$SNAP"
shasum -a 256 "$WORK/delivery.tar" > "$EVID/archive.tar.sha256"
{
  echo "head=$COMMIT"
  echo "tree=$(git -C "$ROOT" rev-parse "${COMMIT}^{tree}")"
  echo "parent=$(git -C "$ROOT" rev-parse "${COMMIT}^")"
  echo "subject=$(git -C "$ROOT" log -1 --format=%s "$COMMIT")"
  echo "lock_blob=$(git -C "$ROOT" rev-parse "${COMMIT}:Cargo.lock")"
  echo "lock_sha256=$(shasum -a 256 "$SNAP/Cargo.lock" | awk '{print $1}')"
  echo "load_rs_blob=$(git -C "$ROOT" rev-parse "${COMMIT}:crates/oo-cli/src/load.rs")"
  echo "load_rs_sha256=$(shasum -a 256 "$SNAP/crates/oo-cli/src/load.rs" | awk '{print $1}')"
  echo "common_sha256=$(shasum -a 256 "$SNAP/crates/oo-cli/tests/common/mod.rs" | awk '{print $1}')"
  echo "query_sha256=$(shasum -a 256 "$SNAP/crates/oo-cli/tests/query.rs" | awk '{print $1}')"
  echo "readme_sha256=$(shasum -a 256 "$SNAP/contracts/cli/query/README.md" | awk '{print $1}')"
  echo "parent_load_blob=$(git -C "$ROOT" rev-parse "${PARENT}:crates/oo-cli/src/load.rs")"
  echo "review_load_blob=$(git -C "$ROOT" rev-parse "3347f5a280da52c70525879b4608170b6b92b523:crates/oo-cli/src/load.rs")"
  echo "parent_archive_sha256=$(git -C "$ROOT" archive --format=tar "$PARENT" | shasum -a 256 | awk '{print $1}')"
  echo "a106_baseline_archive_sha256=179b88c4fb735c276eb09ab20c605189c2ffced7d518c3f5c7ed37efe6511236"
  echo "snapshot_method=git archive of bound commit only"
  echo "overlay=none"
  echo "workspace_head_not_used=$(git -C "$ROOT" rev-parse HEAD)"
} > "$EVID/input-refs.txt"
shasum -a 256 "$EVID/smoke-inside.sh" "$EVID/probe-inside.sh" "$EVID/probe/a111_probe.rs" > "$EVID/harness.sha256"

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$SMOKE/image.txt"
if ! docker network create --label oo.owner=a111 --label oo.run="$UUID" --label oo.kind=rust-builder "$NET" > "$SMOKE/network-create.txt"; then
  printf 'network_create_failed\n' > "$SMOKE/docker-run.exit"
  exit 1
fi
if ! docker volume create --label oo.owner=a111 --label oo.run="$UUID" --label oo.kind=rust-builder "$VOL_CARGO" > "$SMOKE/volume-cargo.txt"; then
  printf 'volume_cargo_failed\n' > "$SMOKE/docker-run.exit"
  exit 1
fi
if ! docker volume create --label oo.owner=a111 --label oo.run="$UUID" --label oo.kind=rust-builder "$VOL_TARGET" > "$SMOKE/volume-target.txt"; then
  printf 'volume_target_failed\n' > "$SMOKE/docker-run.exit"
  exit 1
fi

{
  echo "run_id=$UUID"
  echo "head=$COMMIT"
  echo "network=$NET"
  echo "container=$CNAME"
  echo "platform=linux/arm64"
  echo "toolchain=1.94.0"
  echo "pull=never"
  echo "cap_drop=ALL"
  echo "network_mode=bridge_not_host"
  echo "home_mount=no"
  echo "socket_mount=no"
  echo "shared_cargo_cache=no"
  echo "tmpfs=not_set"
  echo "probe_not_in_snapshot=yes"
} > "$SMOKE/invocation.txt"

if [ -e "$SNAP/crates/oo-cli/tests/a111_probe.rs" ]; then
  echo "probe file leaked into smoke snapshot" >&2
  printf 'snapshot_contaminated\n' > "$SMOKE/docker-run.exit"
  exit 1
fi

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
  --label oo.owner=a111 \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVID}/smoke-inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SMOKE}/out,target=/out" \
  --mount "type=volume,source=${VOL_CARGO},target=/cargo" \
  --mount "type=volume,source=${VOL_TARGET},target=/target" \
  "$IMAGE" \
  bash /inside.sh
smoke_ec=$?
printf '%s\n' "$smoke_ec" > "$SMOKE/docker-run.exit"
docker inspect "$CNAME" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} CapAdd={{json .HostConfig.CapAdd}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}}' > "$SMOKE/inspect.txt" || true
docker inspect "$CNAME" --format '{{json .HostConfig.Tmpfs}}' > "$SMOKE/tmpfs.txt" || true
docker inspect "$CNAME" --format '{{range .Mounts}}{{.Type}} dst={{.Destination}} rw={{.RW}} name={{.Name}}{{println}}{{end}}' > "$SMOKE/mounts.txt" || true
docker rm -f "$CNAME" >/dev/null 2>&1 || true

if [ "$smoke_ec" -ne 0 ]; then
  echo "smoke_failed=$smoke_ec"
  exit "$smoke_ec"
fi

if ! capacity before_probe "$PROBE/capacity.txt"; then
  printf 'not_started\n' > "$PROBE/docker-run.exit"
  exit 2
fi

cp "$EVID/probe/a111_probe.rs" "$SNAP/crates/oo-cli/tests/a111_probe.rs"
shasum -a 256 "$SNAP/crates/oo-cli/tests/a111_probe.rs" > "$PROBE/a111_probe.rs.sha256"
{
  echo "product_after_probe_copy:"
  shasum -a 256 \
    "$SNAP/crates/oo-cli/src/load.rs" \
    "$SNAP/crates/oo-cli/tests/common/mod.rs" \
    "$SNAP/crates/oo-cli/tests/query.rs" \
    "$SNAP/contracts/cli/query/README.md" \
    "$SNAP/Cargo.lock" \
    "$SNAP/crates/oo-cli/Cargo.toml"
  echo "added=crates/oo-cli/tests/a111_probe.rs"
} > "$PROBE/product-stamp.txt"

docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PROBE/image.txt"
{
  echo "run_id=$UUID"
  echo "container=$PNAME"
  echo "network=none"
  echo "fetch=not_run"
  echo "clippy=not_run"
  echo "offline=yes"
  echo "airgap_claim=no"
  echo "cargo_volume=$VOL_CARGO"
  echo "target_volume=$VOL_TARGET"
} > "$PROBE/invocation.txt"

docker run \
  --name "$PNAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network none \
  --label oo.owner=a111 \
  --label oo.kind=rust-builder \
  --label oo.run="$UUID" \
  -e HOME=/tmp \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${EVID}/probe-inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${PROBE}/out,target=/out" \
  --mount "type=volume,source=${VOL_CARGO},target=/cargo" \
  --mount "type=volume,source=${VOL_TARGET},target=/target" \
  "$IMAGE" \
  bash /inside.sh
probe_ec=$?
printf '%s\n' "$probe_ec" > "$PROBE/docker-run.exit"
docker inspect "$PNAME" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} CapDrop={{json .HostConfig.CapDrop}} CapAdd={{json .HostConfig.CapAdd}} SecurityOpt={{json .HostConfig.SecurityOpt}} Memory={{.HostConfig.Memory}} MemorySwap={{.HostConfig.MemorySwap}} NanoCpus={{.HostConfig.NanoCpus}} PidsLimit={{.HostConfig.PidsLimit}}' > "$PROBE/inspect.txt" || true
docker inspect "$PNAME" --format '{{json .HostConfig.Tmpfs}}' > "$PROBE/tmpfs.txt" || true
docker inspect "$PNAME" --format '{{range .Mounts}}{{.Type}} dst={{.Destination}} rw={{.RW}} name={{.Name}}{{println}}{{end}}' > "$PROBE/mounts.txt" || true
docker rm -f "$PNAME" >/dev/null 2>&1 || true
echo "probe_exit=$probe_ec"
exit "$probe_ec"
