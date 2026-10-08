#!/bin/bash
# One private PostgreSQL 17.10 and one 6g Rust builder. Does not pull or prune.
# Origin: a349 chain launch. Owner a389. Archive 6f87942. Alias stays oo-a317-pg.
# After the chain exits 0, the same container runs the product benchmark phase.
set -u
SRC=/Users/redux/puppy-one/open-ontology
UUID=eef831aa-dba4-4af2-a78b-541bc9e0a389
REVIEW=${SRC}/evidence/${UUID}/SP12/product-benchmark
EVIDENCE=${REVIEW}/linux/chain
REPLAY=${SRC}/testing/product-benchmark/replay
CRATE=${SRC}/testing/product-benchmark
ROOT=/tmp/oo-a389-${UUID}
FIXED=6f87942f1c66ba1a9787b7d7194e58a507af63ef
PGBUILDER=be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba
DIALOGUE=e31635b213911c725159a23dbccc8463cc45cdd7
LLM=bde5e4772bb83407de5e9a50f12c120ce5eca30e
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a389-chain
NET=oo-a389-eef831aa-net
CARGO_VOL=oo-a389-eef831aa-cargo
TARGET_VOL=oo-a389-eef831aa-target
READY=${ROOT}/ready
TOKEN=/tmp/oo-customer-demo-20261007/llm-token
WATCH_PID=""

chmod 755 "$CRATE/linux/chain/launch.sh" "$REPLAY/chain/inside.sh" \
  "$REPLAY/chain/extract-libclang.sh" "$REPLAY/chain/watch.sh" "$REPLAY/chain/load.sh" \
  "$REPLAY/chain/membership-watch.sh" "$REPLAY/pg/up.sh" "$REPLAY/pg/pg-wrap.sh" \
  "$CRATE/linux/bench/entry.sh" "$CRATE/linux/bench/inside.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite chain console.log" >&2
  exit 2
fi
# linux/focused is the preserved failing attempt. A later focused-* directory
# with focused.exit 0 is the passing proof. The first failure is not deleted.
FOCUSED_PASS=""
for dir in "$REVIEW"/linux/focused "$REVIEW"/linux/focused-*; do
  if [ -f "$dir/focused.exit" ] && [ "$(tr -d '[:space:]' < "$dir/focused.exit")" = "0" ]; then
    FOCUSED_PASS=$dir
  fi
done
mkdir -p "$EVIDENCE"
if [ -z "$FOCUSED_PASS" ]; then
  echo "focused proof has not passed" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "$FOCUSED_PASS" > "$EVIDENCE/focused-pass.txt"
if docker container inspect oo-a389-focused >/dev/null 2>&1; then
  echo "focused container is still present" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
mkdir -p "$EVIDENCE/phases" "$READY" "$REVIEW/live" "$REVIEW/oracle" "$REVIEW/bench"
cp "$REPLAY/oracle/oracle.rs" "$REVIEW/oracle/oracle.rs"
rm -f "$READY/stop" "$READY/fixture.ready" "$READY/loaded.ready" "$READY/load.failed" "$READY/load.started" \
  "$READY/runtime.ready" "$READY/inspect.done" "$READY/copy.wanted" "$READY/copy.ready" "$READY/copy.failed" \
  "$READY/copy.started" "$READY/side.wanted" "$READY/restarted.ready" "$READY/restart.failed" \
  "$READY/restart.started" "$READY/priv.done" "$READY/priv.started" "$READY/side.failed" "$READY/stats.done"
if [ -e "$REVIEW/live/run" ]; then
  echo "refusing to reuse an existing run directory" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$TOKEN" ]; then
  echo "bearer file is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
bearer_bytes=$(wc -c < "$TOKEN" | tr -d ' ')
printf 'bearer_bytes=%s\n' "$bearer_bytes" > "$REVIEW/bench/bearer-bytes.txt"
if [ "$bearer_bytes" -lt 1 ] || [ "$bearer_bytes" -gt 256 ]; then
  echo "bearer file length is outside the builder range" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
for commit in "$FIXED" "$PGBUILDER" "$DIALOGUE" "$LLM"; do
  if ! git -C "$SRC" cat-file -e "${commit}^{commit}" 2>/dev/null; then
    echo "pinned commit is absent: $commit" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
done
own=$(docker ps -q --filter label=oo.owner=a389 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a389 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s host_cargo=%s own_pg=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ge 1 ]; then
  echo "deferred: builder or postgres capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
rm -rf "$ROOT/archive" "$ROOT/pg-builder" "$ROOT/dialogue" "$ROOT/llm"
mkdir -p "$ROOT/archive" "$ROOT/pg-builder" "$ROOT/dialogue" "$ROOT/llm" "$ROOT/relay"
git -C "$SRC" archive --format=tar "$FIXED" | tar -x -C "$ROOT/archive"
printf '%s\n' "$FIXED" > "$ROOT/archive/.fixed-base-sha"
git -C "$SRC" archive --format=tar "$PGBUILDER" | tar -x -C "$ROOT/pg-builder"
git -C "$SRC" archive --format=tar "$DIALOGUE" | tar -x -C "$ROOT/dialogue"
git -C "$SRC" archive --format=tar "$LLM" | tar -x -C "$ROOT/llm"
umask 077
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$ROOT/relay/key.pem" -out "$ROOT/relay/cert.pem" -days 3 -subj "/CN=127.0.0.1" -addext "subjectAltName=IP:127.0.0.1,DNS:127.0.0.1" >/dev/null 2>&1 \
  || openssl req -x509 -newkey rsa:2048 -nodes -keyout "$ROOT/relay/key.pem" -out "$ROOT/relay/cert.pem" -days 3 -subj "/CN=127.0.0.1" >/dev/null 2>&1
chmod 600 "$ROOT/relay/key.pem"
shasum -a 256 "$ROOT/relay/cert.pem" > "$REVIEW/bench/relay-cert.sha256"
if ! bash "$REPLAY/pg/up.sh"; then
  echo "postgres did not start" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
bash "$REPLAY/chain/watch.sh" > "$EVIDENCE/watch.log" 2>&1 &
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
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a389 --label oo.session="$UUID" --label oo.kind=cargo \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a389 --label oo.session="$UUID" --label oo.kind=target \
  "$TARGET_VOL" >/dev/null
SEC=$(tr -d '[:space:]' < "$REVIEW/pg/secrets-path.txt")
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 --add-host host.docker.internal:host-gateway image=pinned-rust-1.94 bash /entry.sh" > "$EVIDENCE/invocation.txt"
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
  --add-host host.docker.internal:host-gateway \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${SRC}/testing/real-db/pg-e2e,target=/fixture,readonly" \
  --mount "type=bind,source=${REPLAY}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${READY},target=/ready" \
  --mount "type=bind,source=${REPLAY}/chain/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${REPLAY}/chain/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${CRATE}/linux/bench/entry.sh,target=/entry.sh,readonly" \
  --mount "type=bind,source=${CRATE}/linux/bench/inside.sh,target=/bench-inside.sh,readonly" \
  --mount "type=bind,source=${CRATE},target=/crate,readonly" \
  --mount "type=bind,source=${ROOT}/pg-builder,target=/pg-builder-src,readonly" \
  --mount "type=bind,source=${ROOT}/dialogue,target=/dialogue-src,readonly" \
  --mount "type=bind,source=${ROOT}/llm,target=/llm-src,readonly" \
  --mount "type=bind,source=${ROOT}/relay,target=/relay,readonly" \
  --mount "type=bind,source=${TOKEN},target=/run/llm-token,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a389 \
  --label oo.session="$UUID" \
  --label oo.task=product-benchmark-chain \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /entry.sh > "$EVIDENCE/console.log" 2>&1
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
