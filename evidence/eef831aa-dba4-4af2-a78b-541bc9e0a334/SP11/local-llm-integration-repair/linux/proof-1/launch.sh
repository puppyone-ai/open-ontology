#!/bin/bash
# Post-edit proof. Recipe origin is the a334 preoverlay launch, which reused
# a324 proof-10, which reused a204/a200/a199.
# Changes: live crate mount, focused tests, clippy -D warnings, offline, no fetch.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/linux/proof-1
ROOT=/tmp/oo-a334-eef831aa-dba4-4af2-a78b-541bc9e0a334/proof-1
CRATE=${SRC}/testing/real-db/local-llm
FIXED=63566b60aabdf29a29bfe55c4866edf3c4081cfd
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a334-proof1
NET=oo-a334-eef831aa-net
CARGO_VOL=oo-a334-eef831aa-cargo
TARGET_VOL=oo-a334-eef831aa-target
A324_LAUNCH=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a324/SP11/local-llm/linux/proof-10/launch.sh
A324_INSIDE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a324/SP11/local-llm/linux/proof-10/inside.sh

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof-1 console.log" >&2
  exit 2
fi
test -f "$CRATE/Cargo.toml"
test -f "$CRATE/src/token.rs"
test -f "$CRATE/src/stream.rs"
rm -rf "$ROOT/archive"
mkdir -p "$ROOT/archive"
git -C "$SRC" archive --format=tar "$FIXED" Cargo.lock tools/description-provider/Cargo.lock \
  | tar -x -C "$ROOT/archive"
test -f "$ROOT/archive/Cargo.lock"
test -f "$ROOT/archive/tools/description-provider/Cargo.lock"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
{
  echo "a324 launch proof-10 63566b60aabdf29a29bfe55c4866edf3c4081cfd evidence/eef831aa-dba4-4af2-a78b-541bc9e0a324/SP11/local-llm/linux/proof-10/launch.sh $(shasum -a 256 "$A324_LAUNCH" | awk '{print $1}')"
  echo "a324 inside proof-10 63566b60aabdf29a29bfe55c4866edf3c4081cfd evidence/eef831aa-dba4-4af2-a78b-541bc9e0a324/SP11/local-llm/linux/proof-10/inside.sh $(shasum -a 256 "$A324_INSIDE" | awk '{print $1}')"
  echo "a204 launch b2b3adabd05de94723d8ed13b33be0144a0de1f9 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841"
  echo "a200 launch a587031c719a484830d78e493605f48e2056cae4 837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163"
  echo "a199 launch 5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9 bf2a422d751db818cdd4d852d14be00436d4177cb584b85131a4774453413f55"
  echo "a334 preoverlay evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/linux/preoverlay/launch.sh $(shasum -a 256 "$SRC/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/linux/preoverlay/launch.sh" | awk '{print $1}')"
  echo "changes uuid=a334 fixed=63566b60 memory=6g cpus=1 live-crate offline-test clippy-D-warnings no-fetch no-update"
} > "$EVIDENCE/recipe-origins.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a334 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a334 \
  --label oo.kind=network \
  "$NET" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a334 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof-1 container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
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
  --pids-limit 2048 \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${CRATE},target=/crate,readonly" \
  --mount "type=bind,source=${ROOT}/archive,target=/archive,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a334 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a334 \
  --label oo.task=local-llm-proof1 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
} > "$EVIDENCE/hostconfig.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  docker rm "$NAME" > "$EVIDENCE/container-removed.txt"
fi
exit "$run_exit"
