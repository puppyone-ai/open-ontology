#!/bin/bash
# Host launcher for one 6 GiB builder and the owned PostgreSQL.
# bash 3.2. Does not pull, prune, or mount the Docker socket or host home.
# Origin: a274 chain launch, a204/a199/a200 launch shape. Resource names come from the manifest.
set -eu
set -o pipefail

MODE=${1:-}
HERE=$(cd "$(dirname "$0")" && pwd)
DEMO=$(cd "$HERE/.." && pwd)
REPO=$(cd "$DEMO/../.." && pwd)
# shellcheck source=manifest.sh
. "$HERE/manifest.sh"
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
WATCH_PID=""

case "$MODE" in
  public|smoke|restart-smoke) ;;
  *)
    echo "usage: launch.sh public|smoke|restart-smoke" >&2
    exit 2
    ;;
esac
load_deployment "${OO_DEMO_MANIFEST:-}"
bind_resources "$REPO"
export PROJECT OWNER RUN_ID FIXED_SOURCE
mkdir -p "$EVIDENCE/linux"
if ! docker info >/dev/null 2>&1; then
  echo "docker is not available" >&2
  printf '%s\n' 1 > "$EVIDENCE/linux/docker-info.exit"
  exit 1
fi

next_phase() {
  local base n
  base=$1
  if [ ! -d "$EVIDENCE/linux/$base" ]; then
    printf '%s\n' "$EVIDENCE/linux/$base"
    return
  fi
  n=1
  while [ -d "$EVIDENCE/linux/${base}-attempt-$n" ]; do
    n=$((n + 1))
  done
  printf '%s\n' "$EVIDENCE/linux/${base}-attempt-$n"
}

write_exit() {
  printf '%s\n' "$2" > "$1/docker-run.exit"
}

capacity_ok() {
  local phase own global host_cargo
  phase=$1
  own=$(docker ps -q --filter label=oo.owner="$OWNER" --filter label=oo.session="$RUN_ID" --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  global=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
  printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global" "$host_cargo" > "$phase/capacity.txt"
  if [ "$own" -ge 1 ] || [ "$global" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
    echo "deferred: builder capacity own=$own global=$global host_cargo=$host_cargo" >&2
    write_exit "$phase" 3
    exit 3
  fi
}

prepare_archive() {
  local current
  if [ -f "$ROOT/archive/.fixed-base-sha" ]; then
    current=$(tr -d '[:space:]' < "$ROOT/archive/.fixed-base-sha")
    if [ "$current" = "$FIXED_SOURCE" ]; then
      return
    fi
  fi
  rm -rf "$ROOT/archive"
  mkdir -p "$ROOT/archive"
  git -C "$REPO" archive --format=tar "$FIXED_SOURCE" | tar -x -C "$ROOT/archive"
  printf '%s\n' "$FIXED_SOURCE" > "$ROOT/archive/.fixed-base-sha"
  if ! grep -q 'changes_may_have_been_applied' "$ROOT/archive/tools/pg-onboarding/src/outcome.rs"; then
    echo "archive outcome.rs is not the fixed tool" >&2
    return 2
  fi
}

cleanup_watch() {
  if [ -n "$WATCH_PID" ]; then
    kill "$WATCH_PID" 2>/dev/null || true
    wait "$WATCH_PID" 2>/dev/null || true
    WATCH_PID=""
  fi
}
trap cleanup_watch EXIT

check_mounts() {
  local phase type source dest rest
  phase=$1
  if grep -F 'docker.sock' "$phase/mounts.txt" >/dev/null; then
    echo "docker socket is mounted" >&2
    return 1
  fi
  while read -r type source dest rest; do
    if [ "$type" = "bind" ] && [ "$source" = "${HOME}" ]; then
      echo "host home is mounted" >&2
      return 1
    fi
  done < "$phase/mounts.txt"
  return 0
}

require_ours() {
  local kind name state
  kind=$1
  name=$2
  state=$(object_state "$kind" "$name")
  case "$state" in
    ours) ;;
    missing)
      echo "owned $kind $name is absent" >&2
      exit 2
      ;;
    foreign)
      echo "foreign labels on $kind $name; refusing" >&2
      exit 2
      ;;
  esac
}

note_collision() {
  local kind name state
  kind=$1
  name=$2
  state=$(object_state "$kind" "$name")
  case "$state" in
    missing) ;;
    ours)
      echo "resource collision on $kind $name; refusing to overwrite" >&2
      COLLIDED=1
      ;;
    foreign)
      echo "foreign labels on $kind $name; refusing" >&2
      COLLIDED=1
      ;;
  esac
}

refuse_public_collision() {
  local COLLIDED
  COLLIDED=0
  if [ -d "$EVIDENCE/live/run" ]; then
    echo "run directory already exists; refusing to overwrite" >&2
    COLLIDED=1
  fi
  if [ -e "$ROOT" ]; then
    echo "private directory already exists; refusing to overwrite" >&2
    COLLIDED=1
  fi
  note_collision network "$NET"
  note_collision volume "$PG_VOL"
  note_collision volume "$CARGO_VOL"
  note_collision volume "$TARGET_VOL"
  note_collision container "$PG_NAME"
  note_collision container "$BUILD_PUBLIC"
  note_collision container "$BUILD_SMOKE"
  if [ "$COLLIDED" -ne 0 ]; then
    mkdir -p "$EVIDENCE/linux"
    printf '%s\n' "collision $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$EVIDENCE/linux/collision.txt"
    exit 2
  fi
}

run_builder() {
  local name run_mode phase inside_phase sec run_exit mem cpus priv
  name=$1
  run_mode=$2
  phase=$3
  inside_phase=/evidence/linux/$(basename "$phase")
  mkdir -p "$phase" "$ROOT/ready"
  capacity_ok "$phase"
  if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "pinned rust image is not present locally; not pulling" >&2
    write_exit "$phase" 1
    exit 1
  fi
  docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$phase/image.txt"
  if ! grep -q 'arch=arm64' "$phase/image.txt"; then
    echo "rust image is not arm64" >&2
    write_exit "$phase" 1
    exit 1
  fi
  if docker container inspect "$name" >/dev/null 2>&1; then
    echo "named builder $name already exists" >&2
    write_exit "$phase" 2
    exit 2
  fi
  if ! prepare_archive; then
    write_exit "$phase" 2
    exit 2
  fi
  git -C "$REPO" rev-parse "$FIXED_SOURCE" > "$phase/git-rev-parse.txt"
  sec=$(tr -d '[:space:]' < "$EVIDENCE/pg/secrets-path.txt")
  case "$sec" in
    "$ROOT/secrets") ;;
    *)
      echo "secrets path is not the private directory" >&2
      write_exit "$phase" 2
      exit 2
      ;;
  esac
  if [ "$run_mode" = "public" ]; then
    rm -f "$ROOT/ready/runtime.ready"
    bash "$DEMO/fixture/membership-watch.sh" "$ROOT/ready/runtime.ready" "$PG_NAME" > "$phase/membership-watch.log" 2>&1 &
    WATCH_PID=$!
    printf '%s\n' "$WATCH_PID" > "$ROOT/watch.pid"
  fi
  printf '%s\n' "docker run --name ${name} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$phase/invocation.txt"
  set +e
  docker run --name "$name" \
    --platform linux/arm64 \
    --pull never \
    --init \
    --user 0:0 \
    --security-opt no-new-privileges \
    --cap-drop ALL \
    --memory 6g \
    --memory-swap 6g \
    --cpus 1 \
    --pids-limit 4096 \
    --network "$NET" \
    -e HOME=/tmp \
    -e CARGO_HOME=/cargo-home \
    -e CARGO_BUILD_JOBS=1 \
    -e RUSTUP_TOOLCHAIN=1.94.0 \
    -e CARGO_TERM_COLOR=never \
    -e OO_DEMO_MODE="$run_mode" \
    -e OO_DEMO_PHASE="$inside_phase" \
    --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
    --mount "type=bind,source=${DEMO},target=/deploy,readonly" \
    --mount "type=bind,source=${EVIDENCE},target=/evidence" \
    --mount "type=bind,source=${OO_DEMO_MANIFEST},target=/deployment.json,readonly" \
    --mount "type=bind,source=${sec},target=/secrets,readonly" \
    --mount "type=bind,source=${ROOT}/ready,target=/ready" \
    --mount "type=bind,source=${DEMO}/linux/inside.sh,target=/inside.sh,readonly" \
    --mount "type=bind,source=${DEMO}/linux/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
    --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
    --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
    --mount "type=volume,source=${TARGET_VOL},target=/target" \
    --label oo.project="$PROJECT" \
    --label oo.owner="$OWNER" \
    --label oo.session="$RUN_ID" \
    --label oo.fixed="$FIXED_SOURCE" \
    --label oo.task=trusted-demo-${run_mode} \
    --label oo.toolchain=1.94.0 \
    --label oo.platform=linux/arm64 \
    --label oo.kind=rust-builder \
    "$IMAGE" \
    bash /inside.sh > "$phase/console.log" 2>&1
  run_exit=$?
  set -e
  {
    echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$name")"
    echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$name")"
    echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$name")"
    echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$name")"
    echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$name")"
    echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$name")"
    echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$name")"
    echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$name")"
    echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$name")"
    echo "Id=$(docker inspect -f '{{.Id}}' "$name")"
  } > "$phase/hostconfig.txt"
  docker inspect -f '{{json .HostConfig.Tmpfs}}' "$name" > "$phase/tmpfs.txt"
  docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$name" > "$phase/mounts.txt"
  docker rm "$name" > "$phase/container-removed.txt" || true
  cleanup_watch
  if ! check_mounts "$phase"; then
    write_exit "$phase" 1
    exit 1
  fi
  mem=$(awk -F= '/^Memory=/{print $2}' "$phase/hostconfig.txt")
  cpus=$(awk -F= '/^NanoCpus=/{print $2}' "$phase/hostconfig.txt")
  priv=$(awk -F= '/^Privileged=/{print $2}' "$phase/hostconfig.txt")
  if [ "$mem" != "6442450944" ] || [ "$cpus" != "1000000000" ] || [ "$priv" != "false" ]; then
    echo "builder limits were not 6GiB, 1 CPU, and unprivileged" >&2
    write_exit "$phase" 1
    exit 1
  fi
  write_exit "$phase" "$run_exit"
  exit "$run_exit"
}

create_volume() {
  local name kind state
  name=$1
  kind=$2
  state=$(object_state volume "$name")
  case "$state" in
    missing) ;;
    ours)
      echo "resource collision on volume $name; refusing to overwrite" >&2
      exit 2
      ;;
    foreign)
      echo "foreign labels on volume $name; refusing" >&2
      exit 2
      ;;
  esac
  docker volume create \
    --label oo.project="$PROJECT" \
    --label oo.owner="$OWNER" \
    --label oo.session="$RUN_ID" \
    --label oo.fixed="$FIXED_SOURCE" \
    --label oo.kind="$kind" \
    "$name" >/dev/null
}

restart_pg() {
  local state
  echo "restart_postgres"
  state=$(object_state container "$PG_NAME")
  case "$state" in
    ours)
      docker stop "$PG_NAME" >/dev/null
      ;;
    missing) ;;
    foreign)
      echo "foreign labels on container $PG_NAME; refusing" >&2
      exit 2
      ;;
  esac
  if ! bash "$DEMO/pg/up.sh" reuse; then
    echo "postgres did not become ready after restart" >&2
    exit 1
  fi
  if ! bash "$DEMO/fixture/load.sh"; then
    echo "fixture check failed after restart" >&2
    exit 1
  fi
  state=$(tr -d '[:space:]' < "$EVIDENCE/pg/fixture-state.txt")
  printf 'restart_state=%s\n' "$state" > "$EVIDENCE/pg/restart-state.txt"
  if [ "$state" != "present" ]; then
    echo "restart reloaded the fixture; owned rows were not preserved" >&2
    exit 1
  fi
}

if [ "$MODE" = "public" ]; then
  refuse_public_collision
  write_durable
  phase=$(next_phase public)
  mkdir -p "$phase"
  if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "pinned rust image is not present locally; not pulling" >&2
    write_exit "$phase" 1
    exit 1
  fi
  if ! bash "$DEMO/pg/up.sh" create; then
    echo "postgres did not start" >&2
    write_exit "$phase" 1
    exit 1
  fi
  ports=$(tr -d '[:space:]' < "$EVIDENCE/pg/ports.json")
  case "$ports" in
    "{}"|"null") ;;
    *)
      echo "postgres published a host port" >&2
      write_exit "$phase" 1
      exit 1
      ;;
  esac
  if ! bash "$DEMO/fixture/load.sh"; then
    echo "fixture load failed" >&2
    write_exit "$phase" 1
    exit 1
  fi
  create_volume "$CARGO_VOL" cargo
  create_volume "$TARGET_VOL" target
  run_builder "$BUILD_PUBLIC" public "$phase"
fi

if [ "$MODE" = "restart-smoke" ] || [ "$MODE" = "smoke" ]; then
  require_durable
  if [ ! -f "$EVIDENCE/live/receipt.json" ] || ! grep -q '"status":"ready_for_trusted_query"' "$EVIDENCE/live/receipt.json"; then
    echo "smoke requires a ready receipt" >&2
    exit 2
  fi
  require_ours volume "$PG_VOL"
  require_ours volume "$CARGO_VOL"
  require_ours volume "$TARGET_VOL"
  require_ours network "$NET"
  if [ "$MODE" = "restart-smoke" ]; then
    restart_pg
  fi
  phase=$(next_phase smoke)
  mkdir -p "$phase"
  run_builder "$BUILD_SMOKE" smoke "$phase"
fi
