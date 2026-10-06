#!/bin/bash
# Live proof. One 6GiB/1CPU builder after smoke-2 has exited and its container is gone.
# One private Postgres. No prune. Does not overwrite an existing console.log.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a304/SP06/gateway-token-store
PROOF=${EVIDENCE}/linux/proof
ROOT=/tmp/oo-a304-eef831aa-dba4-4af2-a78b-541bc9e0a304/proof
FIXTURE=/tmp/oo-a304-eef831aa-dba4-4af2-a78b-541bc9e0a304/builder-fixture
FIXED=25dfc9022780c3deff12f37b7ad7ea4ed1fb6480
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a304-proof
NET=oo-a304-eef831aa-net
CARGO_VOL=oo-a304-eef831aa-cargo
TARGET_VOL=oo-a304-eef831aa-target
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a304
PHASE=${PROOF}/phase

chmod 755 "$PROOF/inside.sh" "$PROOF/launch.sh" "$PROOF/watch-pg.sh" "$EVIDENCE/pg/up.sh"
if [ -f "$PROOF/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
smoke_exit=$(cat "$EVIDENCE/linux/smoke-2/docker-run.exit" 2>/dev/null || echo missing)
printf '%s\n' "$smoke_exit" > "$PROOF/smoke-2-exit.txt"
if [ "$smoke_exit" != "0" ]; then
  echo "smoke-2 did not pass" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
if docker container inspect oo-a304-smoke-2 >/dev/null 2>&1; then
  echo "smoke-2 container still exists" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PROOF/docker-run.exit"
  exit 1
fi
own=$(docker ps -q --filter label=oo.owner=a304 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 memory=6g cpus=1\n' "$own" "$global_builders" "$host_cargo" > "$PROOF/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$PROOF/docker-run.exit"
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi

if [ "$(docker inspect -f '{{.State.Running}}' oo-a304-pg 2>/dev/null || true)" = "true" ] \
  && docker exec -u postgres oo-a304-pg pg_isready -U oo_fixture_admin -d oo_a304 >/dev/null 2>&1 \
  && test -f "$FIXTURE/ca.crt"; then
  echo "reused running oo-a304-pg" > "$PROOF/pg-up.exit"
  printf '%s\n' 0 >> "$PROOF/pg-up.exit"
else
  bash "$EVIDENCE/pg/up.sh"
  pg_exit=$?
  printf '%s\n' "$pg_exit" > "$PROOF/pg-up.exit"
  if [ "$pg_exit" -ne 0 ]; then
    echo "postgres did not start" >&2
    printf '%s\n' "$pg_exit" > "$PROOF/docker-run.exit"
    exit "$pg_exit"
  fi
fi
test -f "$FIXTURE/ca.crt"
test -f "$FIXTURE/wrong.crt"
test -f "$FIXTURE/admin.pass"
test -f "$FIXTURE/token-admin.pass"
test -f "$FIXTURE/token-runtime.pass"

rm -rf "$ROOT/archive" "$PHASE"
mkdir -p "$ROOT/archive" "$PHASE"
git -C "$SRC" archive --format=tar "$FIXED" Cargo.lock rust-toolchain.toml | tar -x -C "$ROOT/archive"
if [ -e "$ROOT/archive/tools/gateway-token-store" ]; then
  echo "fixed archive already contains gateway-token-store" >&2
  printf '%s\n' 2 > "$PROOF/docker-run.exit"
  exit 2
fi
mkdir -p "$ROOT/archive/tools"
cp -R "$SRC/tools/gateway-token-store" "$ROOT/archive/tools/gateway-token-store"
rm -rf "$ROOT/archive/tools/gateway-token-store/target"
git -C "$SRC" rev-parse HEAD > "$PROOF/repo-head.txt"
printf '%s\n' "$FIXED" > "$PROOF/fixed-commit.txt"
{
  echo "origin_a204_launch evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh"
  echo "origin_a200_launch evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh"
  echo "origin_a200_pg evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/pg/up.sh"
  echo "origin_a199_manifest evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs"
  shasum -a 256 "${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs"
  echo "changes memory_8g_to_6g cpus_4_to_1 jobs_to_1 global_cap_6_to_10 overlay_gateway_token_store subset_before_fetch pg_stop_watcher"
} > "$PROOF/recipe-origin.txt"
cat > "$PROOF/invocation.txt" <<EOF
run_id=${RUN_ID}
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
network=${NET}
container=${NAME}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
phase=proof
memory=6g
cpus=1
global_cap=10
fixed=${FIXED}
archive=${ROOT}/archive
fixture=builder-fixture-without-private-keys
EOF

bash "$PROOF/watch-pg.sh" "$PHASE" > "$PROOF/watcher.log" 2>&1 &
echo $! > "$PROOF/watcher.pid"

set +e
docker run --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges:true \
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
  -e OO_A304_LIVE=1 \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${PROOF}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${FIXTURE},target=/run/oo-a304/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a304 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=gateway-token-store-proof \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$PROOF/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$PROOF/docker-run.exit"
printf '%s\n' "$run_exit" > "$PHASE/builder.exit"
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$NAME" 2>/dev/null || echo missing)"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$NAME" 2>/dev/null || echo missing)"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$NAME" 2>/dev/null || echo missing)"
  echo "ReadonlyRootfs=$(docker inspect -f '{{.HostConfig.ReadonlyRootfs}}' "$NAME" 2>/dev/null || echo missing)"
  echo "Id=$(docker inspect -f '{{.Id}}' "$NAME" 2>/dev/null || echo missing)"
} > "$PROOF/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$NAME" > "$PROOF/tmpfs.txt" 2>/dev/null || true
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$NAME" > "$PROOF/mounts.txt" 2>/dev/null || true
docker rm "$NAME" > "$PROOF/container-removed.txt" 2>&1 || true
if [ -f "$PROOF/watcher.pid" ]; then
  wait "$(cat "$PROOF/watcher.pid")" || true
fi
exit "$run_exit"
