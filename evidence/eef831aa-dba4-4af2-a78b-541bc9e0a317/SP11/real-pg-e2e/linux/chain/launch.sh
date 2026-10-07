#!/bin/bash
# One private PostgreSQL 17.10 and one 6g Rust builder. Does not pull or prune.
# Product source is the immutable archive. The fixture driver is mounted separately.
# Origin: a274 chain launch, from a204 overlay 78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee,
# a199 overlay c837e9d18e1ae9fa9ae90d03f920d645f9b30d235a2384e630aa0a9c9782b69a,
# and a200 proof c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541.
# Change: owner a317, archive 10ecd765, alias oo-a317-pg, 100000-row fixture, separate driver.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a317/SP11/real-pg-e2e
EVIDENCE=${REVIEW}/linux/chain
ROOT=/tmp/oo-a317-eef831aa-dba4-4af2-a78b-541bc9e0a317
FIXED=10ecd7654fd7939b6655a0e50a84214a7fca316e
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a317-chain
NET=oo-a317-eef831aa-net
CARGO_VOL=oo-a317-eef831aa-cargo
TARGET_VOL=oo-a317-eef831aa-target
READY=${ROOT}/ready
WATCH_PID=""

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh" "$EVIDENCE/extract-libclang.sh" "$EVIDENCE/watch.sh" \
  "$REVIEW/pg/up.sh" "$REVIEW/pg/pg-wrap.sh" \
  "$SRC/testing/real-db/pg-e2e/cluster/"*.sh
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite chain console.log" >&2
  exit 2
fi
if [ ! -f "$REVIEW/linux/focused/focused.exit" ] || [ "$(tr -d '[:space:]' < "$REVIEW/linux/focused/focused.exit")" != "0" ]; then
  echo "focused proof has not passed" >&2
  mkdir -p "$EVIDENCE"
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if docker container inspect oo-a317-focused >/dev/null 2>&1; then
  echo "focused container is still present" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
mkdir -p "$EVIDENCE/phases" "$READY" "$REVIEW/live"
rm -f "$READY/stop" "$READY/fixture.ready" "$READY/loaded.ready" "$READY/load.failed" "$READY/load.started" \
  "$READY/runtime.ready" "$READY/inspect.done" "$READY/copy.wanted" "$READY/copy.ready" "$READY/copy.failed" \
  "$READY/copy.started" "$READY/side.wanted" "$READY/restarted.ready" "$READY/restart.failed" \
  "$READY/restart.started" "$READY/priv.done" "$READY/priv.started" "$READY/side.failed" "$READY/stats.done"
if [ -e "$REVIEW/live/run" ]; then
  echo "refusing to reuse an existing run directory" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a317 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a317 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s host_cargo=%s own_pg=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ge 1 ]; then
  echo "deferred: builder or postgres capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" | tar -x -C "$ROOT/archive"
printf '%s\n' "$FIXED" > "$ROOT/archive/.fixed-base-sha"
if ! bash "$REVIEW/pg/up.sh"; then
  echo "postgres did not start" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
bash "$EVIDENCE/watch.sh" > "$EVIDENCE/watch.log" 2>&1 &
WATCH_PID=$!
cleanup_watch() {
  : > "$READY/stop"
  if [ -n "$WATCH_PID" ]; then
    kill "$WATCH_PID" 2>/dev/null || true
    wait "$WATCH_PID" 2>/dev/null || true
  fi
  if [ -f "$EVIDENCE/membership.pid" ]; then
    kill "$(cat "$EVIDENCE/membership.pid")" 2>/dev/null || true
  fi
}
trap cleanup_watch EXIT
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || { echo "image is not arm64" >&2; printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"; exit 1; }
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named chain container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
SEC=$(tr -d '[:space:]' < "$REVIEW/pg/secrets-path.txt")
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
set +e
docker run --name "$NAME" \
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
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${SRC}/testing/real-db/pg-e2e,target=/fixture,readonly" \
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${READY},target=/ready" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a317 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a317 \
  --label oo.task=real-pg-e2e-chain \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME")"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME")"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME")"
} > "$EVIDENCE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$EVIDENCE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$EVIDENCE/mounts.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
cleanup_watch
WATCH_PID=""
exit "$run_exit"
