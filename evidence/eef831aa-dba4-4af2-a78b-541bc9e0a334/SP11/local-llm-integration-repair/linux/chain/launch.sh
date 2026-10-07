#!/bin/bash
# One dialogue propose through the host relay. Archive is the fixed commit.
# Origin shape: a300 proof-8 launch. Changes: no dialogue overlay, a334 names,
# 6g/1cpu, client-token mount, no upstream token, no oauth, no host network.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair
PROOF=${EVIDENCE}/linux/chain
ROOT=/tmp/oo-a334-eef831aa-dba4-4af2-a78b-541bc9e0a334/chain
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=63566b60aabdf29a29bfe55c4866edf3c4081cfd
NAME=oo-a334-chain
NET=oo-a334-eef831aa-net
CARGO_VOL=oo-a334-eef831aa-cargo
TARGET_VOL=oo-a334-eef831aa-dialogue-target
CLIENT=/tmp/oo-a334-eef831aa-dba4-4af2-a78b-541bc9e0a334/host/client-token
CA=${EVIDENCE}/host/ca.pem
A300=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a300/SP11/ontology-dialogue/linux/proof-8/launch.sh

mkdir -p "$PROOF/phases" "$ROOT"
chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh" "$EVIDENCE/pg/up.sh" "$EVIDENCE/pg/pg-wrap.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite chain console.log" >&2
  exit 2
fi
test -f "$CLIENT"
test -f "$CA"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a334 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$PROOF/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named chain container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
{
  echo "a300 proof-8 launch 96afe70fd68a56ae8eec7504bf20e09ecd3cdf65 ${A300} $(shasum -a 256 "$A300" | awk '{print $1}')"
  echo "fixed=${FIXED}"
  echo "changes no-overlay archive-includes-dialogue client-token-mount no-upstream-token memory=6g cpus=1"
} > "$PROOF/recipe-origins.txt"
PROOF_SRC=${ROOT}/archive
rm -rf "$PROOF_SRC"
mkdir -p "$PROOF_SRC"
git -C "$SRC" archive "$FIXED" | tar -x -C "$PROOF_SRC"
test -f "$PROOF_SRC/tools/ontology-dialogue/src/lib.rs"
test -f "$PROOF_SRC/crates/oo-gateway/src/bin/query-pg.rs"
printf '%s\n' "$FIXED" > "$PROOF/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$PROOF/fixed-tree.txt"
if ! docker ps --filter "name=^/oo-a334-pg$" --filter status=running --format '{{.Names}}' | grep -qx oo-a334-pg; then
  bash "$EVIDENCE/pg/up.sh" > "$PROOF/postgres-up.stdout" 2> "$PROOF/postgres-up.stderr"
  pg_up=$?
  printf '%s\n' "$pg_up" > "$PROOF/postgres-up.exit"
  if [ "$pg_up" -ne 0 ]; then
    echo "postgres did not start" >&2
    printf '%s\n' "$pg_up" > "$PROOF/docker-run.exit"
    exit "$pg_up"
  fi
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$PROOF/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94" > "$PROOF/invocation.txt"
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
  --add-host host.docker.internal:host-gateway \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${PROOF_SRC},target=/src,readonly" \
  --mount "type=bind,source=${PROOF},target=/evidence/linux/chain" \
  --mount "type=bind,source=${CA},target=/run/relay-ca.pem,readonly" \
  --mount "type=bind,source=${CLIENT},target=/run/client-token,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  --tmpfs /work:rw,nosuid,nodev,size=64m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a334 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a334 \
  --label oo.task=local-llm-dialogue-propose \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /evidence/linux/chain/inside.sh > "$PROOF/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROOF/docker-run.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
} > "$PROOF/hostconfig.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  docker rm "$NAME" > "$PROOF/container-removed.txt"
fi
exit "$run_exit"
