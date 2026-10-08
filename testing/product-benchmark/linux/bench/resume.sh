#!/bin/bash
# Resume the model benchmark on the existing 100k database.
# Does not start PostgreSQL, does not reload the fixture, does not rebuild the chain.
# The container is detached so a client hangup does not signal it.
set -u
SRC=/Users/redux/puppy-one/open-ontology
UUID=eef831aa-dba4-4af2-a78b-541bc9e0a389
REVIEW=${SRC}/evidence/${UUID}/SP12/product-benchmark
EVIDENCE=${REVIEW}/linux/bench-resume
ROOT=/tmp/oo-a389-${UUID}
CRATE=${SRC}/testing/product-benchmark
REPLAY=${SRC}/testing/product-benchmark/replay
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a389-bench
NET=oo-a389-eef831aa-net
CARGO_VOL=oo-a389-eef831aa-cargo
TARGET_VOL=oo-a389-eef831aa-target
TOKEN=/tmp/oo-customer-demo-20261007/llm-token
READY=${ROOT}/ready

mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite bench-resume console.log" >&2
  exit 2
fi
if [ "$(tr -d '[:space:]' < "$REVIEW/linux/chain/chain.exit")" != "0" ]; then
  echo "chain proof is not exit 0" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker inspect oo-a389-pg --format '{{.State.Running}}' 2>/dev/null | grep -q true; then
  echo "owned postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if docker ps -q --filter label=oo.owner=a389 --filter label=oo.kind=rust-builder | grep -q .; then
  echo "an owned builder is already running" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
own=$(docker ps -q --filter label=oo.owner=a389 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
own_pg=$(docker ps -q --filter label=oo.owner=a389 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own=%s global=%s host_cargo=%s own_pg=%s cap_global=10\n' "$own" "$global_builders" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ne 1 ]; then
  echo "deferred: builder or postgres capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
for dir in archive pg-builder dialogue llm relay secrets; do
  if [ ! -d "$ROOT/$dir" ]; then
    echo "missing private input $dir" >&2
    printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
    exit 2
  fi
done
if [ ! -f "$TOKEN" ]; then
  echo "bearer file is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
FAIL=${REVIEW}/failed-attempts/bench-sigterm-143
if [ ! -d "$FAIL" ]; then
  mkdir -p "$FAIL"
  cp -a "$REVIEW/bench" "$FAIL/bench"
  cp "$REVIEW/linux/chain/console.log" "$FAIL/chain-console.log"
  docker inspect oo-a389-chain > "$FAIL/chain-container.json" 2>/dev/null || true
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named bench container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
SEC=$(tr -d '[:space:]' < "$REVIEW/pg/secrets-path.txt")
printf '%s\n' "docker run -d --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 --add-host host.docker.internal:host-gateway image=pinned-rust-1.94 bash /bench-inside.sh" > "$EVIDENCE/invocation.txt"
cid=$(docker run -d --name "$NAME" \
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
  --label oo.task=product-benchmark-bench \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /bench-inside.sh)
printf '%s\n' "$cid" > "$EVIDENCE/container.id"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} Memory={{.HostConfig.Memory}} NanoCpus={{.HostConfig.NanoCpus}}' > "$EVIDENCE/hostconfig.txt"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "$EVIDENCE/caps.json"
echo "bench_resumed id=$cid"
