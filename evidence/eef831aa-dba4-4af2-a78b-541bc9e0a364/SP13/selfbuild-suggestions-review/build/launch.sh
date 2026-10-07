#!/bin/bash
# One arm64 Rust 1.94 builder. Flags from
# testing/real-db/gateway-tls/linux/proof/launch.sh. Private a364 names.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a364/SP13/selfbuild-suggestions-review/build
PROBE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a364/SP13/selfbuild-suggestions-review/probe
CATALOG=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a349/SP11/real-pg-100k-review/live/run
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a364-sbs-test
NET=oo-a364-sbs-net
CARGO_VOL=oo-a364-sbs-cargo
TARGET_VOL=oo-a364-sbs-target

own=$(docker ps -q --filter label=oo.owner=a364 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" | tee "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named builder already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create --driver bridge "$NET" > "$EVIDENCE/network.txt"
docker volume create --label oo.owner=a364 --label oo.kind=cargo "$CARGO_VOL" >/dev/null
docker volume create --label oo.owner=a364 --label oo.kind=target "$TARGET_VOL" >/dev/null
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94" > "$EVIDENCE/invocation.txt"
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
  --label oo.kind=rust-builder \
  --label oo.owner=a364 \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -v "${CARGO_VOL}:/cargo-home" \
  -v "${TARGET_VOL}:/target" \
  -v "${SRC}/tools/selfbuild-suggestions:/work/tools/selfbuild-suggestions:ro" \
  -v "${CATALOG}:/work/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a349/SP11/real-pg-100k-review/live/run:ro" \
  -v "${PROBE}:/probe:ro" \
  -v "${EVIDENCE}:/out" \
  "$IMAGE" \
  bash /out/inside.sh
rc=$?
docker inspect "$NAME" --format 'exit={{.State.ExitCode}} oom={{.State.OOMKilled}} status={{.State.Status}}' > "$EVIDENCE/hostconfig.txt"
docker rm "$NAME" >/dev/null
printf '%s\n' "$rc" > "$EVIDENCE/docker-run.exit"
exit "$rc"
