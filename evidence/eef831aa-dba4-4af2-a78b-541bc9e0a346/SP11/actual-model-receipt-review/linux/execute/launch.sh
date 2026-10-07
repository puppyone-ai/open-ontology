#!/bin/bash
# One builder prepares oo-a346-pg with the reviewed live test, then runs one
# approved dialogue execute. The launch refuses to start unless the operator
# decision already says approved, not automatic, and not rewritten.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a346/SP11/actual-model-receipt-review
PROOF=${EVIDENCE}/linux/execute
PROPOSE=${EVIDENCE}/linux/propose
ROOT=/var/tmp/oo-a346-eef831aa-dba4-4af2-a78b-541bc9e0a346/execute
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
FIXED=63566b60aabdf29a29bfe55c4866edf3c4081cfd
NAME=oo-a346-execute
NET=oo-a346-eef831aa-net
CARGO_VOL=oo-a346-eef831aa-cargo
TARGET_VOL=oo-a346-eef831aa-target
FIXTURE=/var/tmp/oo-a346-eef831aa-dba4-4af2-a78b-541bc9e0a346/fixture
CLIENT=/var/tmp/oo-a346-eef831aa-dba4-4af2-a78b-541bc9e0a346/host/client-token
UPSTREAM=/tmp/oo-customer-demo-20261007/llm-token
A334=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a334/SP11/local-llm-integration-repair/linux/execute-3/launch.sh
A332_CLANG=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a332/SP11/ontology-dialogue-review/linux/libclang-extract.sh

mkdir -p "$PROOF/phases" "$PROOF/live" "$ROOT"
chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh" "$EVIDENCE/linux/libclang-extract.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite execute console.log" >&2
  exit 2
fi
decision=${PROPOSE}/operator-decision.txt
if [ ! -f "$decision" ] || [ ! -f "$PROPOSE/approval.json" ] || [ ! -f "$PROPOSE/proposal.json" ] || [ ! -f "$PROPOSE/pack.json" ]; then
  echo "approval is not present; refusing to execute" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if ! grep -q '^approved=true$' "$decision" || ! grep -q '^automatic=false$' "$decision" || ! grep -q '^rewritten=false$' "$decision"; then
  echo "operator decision is not an explicit non-automatic approval" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
test -f "$FIXTURE/ca.crt"
test -f "$FIXTURE/admin.pass"
test -f "$FIXTURE/login.pass"
test -f "$CLIENT"
test -f "$UPSTREAM"
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
  echo "named execute container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if ! docker ps --filter "name=^/oo-a346-pg$" --filter status=running --format '{{.Names}}' | grep -qx oo-a346-pg; then
  echo "postgres is not running; refusing to start a second database from the execute launch" >&2
  printf '%s\n' 2 > "$PROOF/postgres-up.exit"
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
echo "already-running" > "$PROOF/postgres-up.stdout"
printf '%s\n' 0 > "$PROOF/postgres-up.exit"
{
  echo "a334 execute-3 launch ${A334} $(shasum -a 256 "$A334" | awk '{print $1}')"
  echo "a332 libclang ${A332_CLANG} $(shasum -a 256 "$A332_CLANG" | awk '{print $1}')"
  echo "fixed=${FIXED}"
  echo "changes sess-a346 no-sql-rewrite pids=4096 no-tmpfs rust-package-selector operator-decision-gate"
} > "$PROOF/recipe-origins.txt"
PROOF_SRC=${ROOT}/archive
rm -rf "$PROOF_SRC"
mkdir -p "$PROOF_SRC"
git -C "$SRC" archive "$FIXED" | tar -x -C "$PROOF_SRC"
test -f "$PROOF_SRC/tools/ontology-dialogue/src/lib.rs"
test -f "$PROOF_SRC/crates/oo-gateway/src/bin/query-pg.rs"
rm -f "$PROOF_SRC/rust-toolchain.toml"
printf '%s\n' "omitted from the private archive copy; image toolchain is 1.94.0" > "$PROOF/toolchain-omitted.txt"
printf '%s\n' "$FIXED" > "$PROOF/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$PROOF/fixed-tree.txt"
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
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  --mount "type=bind,source=${PROOF_SRC},target=/src,readonly" \
  --mount "type=bind,source=${PROOF},target=/evidence/linux/execute" \
  --mount "type=bind,source=${PROPOSE},target=/evidence/linux/propose,readonly" \
  --mount "type=bind,source=${EVIDENCE}/linux/libclang-extract.sh,target=/libclang-extract.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/tools,target=/tools,readonly" \
  --mount "type=bind,source=${FIXTURE},target=/run/fixture,readonly" \
  --mount "type=bind,source=${CLIENT},target=/run/client-token,readonly" \
  --mount "type=bind,source=${UPSTREAM},target=/run/upstream-token,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a346 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a346 \
  --label oo.task=actual-model-receipt-execute \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /evidence/linux/execute/inside.sh > "$PROOF/console.log" 2>&1
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
