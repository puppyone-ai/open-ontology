#!/bin/bash
# One new dialogue propose. Archive is 63566b60. The observer is already listening.
# No approval is written here. No query-pg. No auth-store mount.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a346/SP11/actual-model-receipt-review
PROOF=${EVIDENCE}/linux/propose
ROOT=/var/tmp/oo-a346-eef831aa-dba4-4af2-a78b-541bc9e0a346/propose
HOST=/var/tmp/oo-a346-eef831aa-dba4-4af2-a78b-541bc9e0a346/host
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=63566b60aabdf29a29bfe55c4866edf3c4081cfd
NAME=oo-a346-propose
NET=oo-a346-eef831aa-net
CARGO_VOL=oo-a346-eef831aa-cargo
TARGET_VOL=oo-a346-eef831aa-target
CLIENT=${HOST}/client-token
UPSTREAM=/tmp/oo-customer-demo-20261007/llm-token
CA=${EVIDENCE}/host/ca.pem
OBSERVE_SRC=${HOST}/observe-src
OBSERVE_OUT=${HOST}/observe-out
A334=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/linux/chain-4/launch.sh

mkdir -p "$PROOF/phases" "$ROOT"
chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite propose console.log" >&2
  exit 2
fi
if [ -f "$PROOF/approval.json" ]; then
  echo "approval already exists; refusing to propose after approval" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
test -f "$CLIENT"
test -f "$UPSTREAM"
test -f "$CA"
test -f "$OBSERVE_SRC/src/bin/observe.rs"
test -d "$OBSERVE_OUT"
if [ ! -f "$EVIDENCE/host/smoke.exit" ] || [ "$(cat "$EVIDENCE/host/smoke.exit")" != 0 ]; then
  echo "relay smoke is not green; no model call" >&2
  printf '%s\n' 4 > "$PROOF/docker-run.exit"
  exit 4
fi
if ! grep -q '"model_available":true' "$EVIDENCE/host/models.json" || ! grep -q '"selected_model_id":"grok-4.7"' "$EVIDENCE/host/models.json"; then
  echo "grok-4.7 gate is closed; no model call" >&2
  printf '%s\n' 4 > "$PROOF/docker-run.exit"
  exit 4
fi
if ! lsof -nP -iTCP:18446 -sTCP:LISTEN >/dev/null 2>&1 || ! lsof -nP -iTCP:18447 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "relay or observer is not listening; no model call" >&2
  printf '%s\n' 4 > "$PROOF/docker-run.exit"
  exit 4
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi
own_docker=$(docker ps -q --filter label=oo.owner=a346 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_docker=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_host=0
global_host=0
while read -r pid comm; do
  [ -n "${pid:-}" ] || continue
  global_host=$((global_host + 1))
  cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | awk '/^n/ {print substr($0,2); exit}')
  case "$cwd" in
    *oo-a346*) own_host=$((own_host + 1)) ;;
  esac
done <<EOF
$(ps -axo pid=,comm= | awk '$2=="cargo" || $2=="rustc" || $2=="rustdoc" || $2=="rustup" {print $1, $2}')
EOF
own=$((own_docker + own_host))
global=$((global_docker + global_host))
printf 'own=%s global=%s own_docker=%s own_host=%s global_docker=%s global_host=%s cap_global=10 cap_own=1\n' \
  "$own" "$global" "$own_docker" "$own_host" "$global_docker" "$global_host" > "$PROOF/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 10 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named propose container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
{
  echo "a334 chain-4 launch ${A334} $(shasum -a 256 "$A334" | awk '{print $1}')"
  echo "fixed=${FIXED}"
  echo "changes new-question observer-18447 clippy-before-http no-tmpfs pids=4096 no-approval no-execute"
} > "$PROOF/recipe-origins.txt"
PROOF_SRC=${ROOT}/archive
rm -rf "$PROOF_SRC"
mkdir -p "$PROOF_SRC"
git -C "$SRC" archive "$FIXED" | tar -x -C "$PROOF_SRC"
test -f "$PROOF_SRC/tools/ontology-dialogue/src/lib.rs"
rm -f "$PROOF_SRC/rust-toolchain.toml"
printf '%s\n' "omitted from the private archive copy; image toolchain is 1.94.0" > "$PROOF/toolchain-omitted.txt"
printf '%s\n' "$FIXED" > "$PROOF/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$PROOF/fixed-tree.txt"
git -C "$SRC" rev-parse "${FIXED}:tools/ontology-dialogue" > "$PROOF/dialogue-tree.txt"
grep -q '^1941b8a6a885ec85ac93da6cc6f4d45c8ecb48a0$' "$PROOF/dialogue-tree.txt"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$PROOF/image.txt"
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 --pids-limit 4096 image=pinned-rust-1.94" > "$PROOF/invocation.txt"
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
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${PROOF_SRC},target=/src,readonly" \
  --mount "type=bind,source=${OBSERVE_SRC},target=/relay,readonly" \
  --mount "type=bind,source=${OBSERVE_OUT},target=/observe-out,readonly" \
  --mount "type=bind,source=${PROOF},target=/evidence/linux/propose" \
  --mount "type=bind,source=${CA},target=/run/relay-ca.pem,readonly" \
  --mount "type=bind,source=${CLIENT},target=/run/client-token,readonly" \
  --mount "type=bind,source=${UPSTREAM},target=/run/upstream-token,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a346 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a346 \
  --label oo.task=actual-model-receipt-propose \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /evidence/linux/propose/inside.sh > "$PROOF/console.log" 2>&1
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
  echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$NAME" 2>/dev/null || echo missing)"
  echo "PortBindings=$(docker inspect -f '{{json .HostConfig.PortBindings}}' "$NAME" 2>/dev/null || echo missing)"
} > "$PROOF/hostconfig.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  docker rm "$NAME" > "$PROOF/container-removed.txt"
fi
exit "$run_exit"
