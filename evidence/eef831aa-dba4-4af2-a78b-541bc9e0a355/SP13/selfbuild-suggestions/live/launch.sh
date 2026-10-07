#!/bin/bash
# One arm64 Rust 1.94 builder for the ignored gateway test.
# Starts only after the query-pg builder has exited. Does not pull or prune.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a355/SP13/selfbuild-suggestions/live
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a355-sbs-live
NET=oo-a355-sbs-net
CARGO_VOL=oo-a355-sbs-cargo
TARGET_VOL=oo-a355-sbs-target
GW_VOL=oo-a355-sbs-gw-target
SECRETS=/tmp/oo-a355-sbs-secrets
QUERYPG=oo-a355-sbs-querypg

if docker container inspect "$QUERYPG" >/dev/null 2>&1; then
  state=$(docker inspect -f '{{.State.Status}}' "$QUERYPG")
  if [ "$state" = "running" ]; then
    echo "query-pg builder is still running" >&2
    printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
    exit 3
  fi
fi
own=$(docker ps -q --filter label=oo.owner=a355 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" | tee "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named builder already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94" > "$EVIDENCE/invocation.txt"
exec docker run --name "$NAME" \
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
  --label oo.kind=rust-builder \
  --label oo.owner=a355 \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_A355_QUERY_PG=/gw-target/debug/query-pg \
  -e OO_A355_REQUEST=/secrets/request.json \
  -v "${CARGO_VOL}:/cargo-home" \
  -v "${TARGET_VOL}:/target" \
  -v "${GW_VOL}:/gw-target:ro" \
  -v "${SECRETS}:/secrets:ro" \
  -v "${SRC}/tools/selfbuild-suggestions:/work/tools/selfbuild-suggestions" \
  -v "${SRC}/evidence:/work/evidence:ro" \
  -v "${EVIDENCE}:/out" \
  "$IMAGE" \
  bash /out/inside.sh
