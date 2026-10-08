#!/bin/bash
# One more pg-builder generate against the cache the second call stored.
# Does not reload the 100k database and does not rebuild the chain.
set -u
SRC=/Users/redux/puppy-one/open-ontology
UUID=eef831aa-dba4-4af2-a78b-541bc9e0a389
REVIEW=${SRC}/evidence/${UUID}/SP12/product-benchmark
EVIDENCE=${REVIEW}/linux/hit
ROOT=/tmp/oo-a389-${UUID}
CRATE=${SRC}/testing/product-benchmark
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a389-hit
NET=oo-a389-eef831aa-net
CARGO_VOL=oo-a389-eef831aa-cargo
TOKEN=/tmp/oo-customer-demo-20261007/llm-token

mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite hit console.log" >&2
  exit 2
fi
if docker ps -q --filter label=oo.kind=rust-builder | grep -q .; then
  echo "a rust builder is already running" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if [ ! -f "$REVIEW/score-input/builder-hit.json" ]; then
  echo "second generate document is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp "$REVIEW/score-input/builder-hit.json" "$REVIEW/live/builder/second-partial.json"
if [ ! -f "$TOKEN" ]; then
  echo "bearer file is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named hit container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /hit-inside.sh" > "$EVIDENCE/invocation.txt"
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
  --pids-limit 256 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  --mount "type=bind,source=${REVIEW},target=/evidence" \
  --mount "type=bind,source=${CRATE},target=/crate,readonly" \
  --mount "type=bind,source=${TOKEN},target=/run/llm-token,readonly" \
  --mount "type=bind,source=${CRATE}/linux/bench/hit-inside.sh,target=/hit-inside.sh,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=256m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --label oo.project=open-ontology \
  --label oo.owner=a389 \
  --label oo.session="$UUID" \
  --label oo.task=product-benchmark-hit \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /hit-inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}} OOMKilled={{.State.OOMKilled}} Privileged={{.HostConfig.Privileged}} NetworkMode={{.HostConfig.NetworkMode}} Memory={{.HostConfig.Memory}}' > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
exit "$run_exit"
