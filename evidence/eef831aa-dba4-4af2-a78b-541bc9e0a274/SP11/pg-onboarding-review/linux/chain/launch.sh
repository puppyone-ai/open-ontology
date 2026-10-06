#!/bin/bash
# One private PostgreSQL 17.10 and one 6g Rust builder. Does not pull or prune.
# Product source and the onboarding tool are the immutable archive.
set -u
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review
EVIDENCE=${REVIEW}/linux/chain
ROOT=/tmp/oo-a274-eef831aa-dba4-4af2-a78b-541bc9e0a274
FIXED=9951cda7f128e30f1f6ce02e409d542d20a9e4fb
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a274-chain
NET=oo-a274-eef831aa-net
CARGO_VOL=oo-a274-eef831aa-cargo
TARGET_VOL=oo-a274-eef831aa-target
READY=${ROOT}/ready/runtime.ready
WATCH_PID=""

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh" "$EVIDENCE/extract-libclang.sh" \
  "$REVIEW/pg/up.sh" "$REVIEW/pg/pg-wrap.sh" \
  "$REVIEW/fixture/load.sh" "$REVIEW/fixture/inside-cluster.sh" "$REVIEW/fixture/grant-runtime.sh" \
  "$REVIEW/fixture/membership-watch.sh"
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
if docker container inspect oo-a274-focused >/dev/null 2>&1; then
  echo "focused container is still present" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
mkdir -p "$EVIDENCE/phases" "$ROOT/ready" "$REVIEW/live"
rm -f "$READY"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a274 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a274 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
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
if ! cmp -s "$ROOT/archive/tools/pg-onboarding/src/outcome.rs" "$REVIEW/source/blobs/outcome.rs"; then
  echo "archive tool drifted from the sealed blob" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! bash "$REVIEW/pg/up.sh"; then
  echo "postgres did not start" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if ! bash "$REVIEW/fixture/load.sh"; then
  echo "fixture load failed" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
SEC=$(tr -d '[:space:]' < "$REVIEW/pg/secrets-path.txt")
bash "$REVIEW/fixture/membership-watch.sh" "$READY" > "$EVIDENCE/membership-watch.log" 2>&1 &
WATCH_PID=$!
cleanup_watch() {
  if [ -n "$WATCH_PID" ]; then
    kill "$WATCH_PID" 2>/dev/null || true
    wait "$WATCH_PID" 2>/dev/null || true
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
  --mount "type=bind,source=${REVIEW}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${ROOT}/ready,target=/ready" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a274 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a274 \
  --label oo.task=pg-onboarding-review-chain \
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
