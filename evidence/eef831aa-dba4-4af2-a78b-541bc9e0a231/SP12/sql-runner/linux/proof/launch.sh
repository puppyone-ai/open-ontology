#!/bin/bash
# One Linux arm64 Rust 1.94 builder plus the already started private Postgres.
# Immutable git archive plus the sql-runner overlay. Does not overwrite console.log.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/linux/proof/launch.sh
# sha256 e1d6556a8c227ee942f502beb7c1efdfcacd11ed271ee1e81a15608064146463
# Delta: owner a231, fixed 934c8098703b6462c4f774878d1a4241bd84905d, overlay benchmarks/sql-runner,
# require the crate lock, no tee wrapper, host flag files for the unmapped table and the lock.
set -u
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/linux/proof
ROOT=/tmp/oo-a231-eef831aa-dba4-4af2-a78b-541bc9e0a231/proof
SEC=/tmp/oo-a231-eef831aa-dba4-4af2-a78b-541bc9e0a231/secrets
FIXED=934c8098703b6462c4f774878d1a4241bd84905d
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NAME=oo-a231-proof
PGNAME=oo-a231-pg
NET=oo-a231-eef831aa-net
CARGO_VOL=oo-a231-eef831aa-cargo
TARGET_VOL=oo-a231-eef831aa-target
TOOLS=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/tools

chmod 755 "$EVIDENCE/inside.sh" "$EVIDENCE/launch.sh" "$EVIDENCE/extract-libclang.sh"
mkdir -p "$EVIDENCE/phases" "$EVIDENCE/admin" "$EVIDENCE/reports" "$EVIDENCE/attempts"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite proof console.log" >&2
  exit 2
fi
if [ ! -f "$SEC/ca.crt" ] || [ ! -f "$SEC/login.pass" ] || [ ! -f "$SEC/admin.pass" ] || [ ! -f "$SEC/other.crt" ]; then
  echo "fixture secrets are missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "${SRC}/benchmarks/sql-runner/Cargo.lock" ]; then
  echo "sql-runner Cargo.lock is missing" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
pg_state=$(docker inspect -f '{{.State.Running}}' "$PGNAME" 2>/dev/null || echo missing)
printf '%s\n' "$pg_state" > "$EVIDENCE/postgres-running.txt"
if [ "$pg_state" != "true" ]; then
  echo "owned postgres is not running" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a231 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a231 \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a231 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a231 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a231 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a231 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null

rm -rf "$ROOT/archive" "$ROOT/proof-src"
mkdir -p "$ROOT/archive" "$ROOT/proof-src"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock rust-toolchain.toml .cargo crates \
  benchmarks/query-result-adapter benchmarks/result-comparator \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/benchmarks/result-comparator/Cargo.lock" ]; then
  echo "archive missing result-comparator lock" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/benchmarks/query-result-adapter/Cargo.lock" ]; then
  echo "archive missing query-result-adapter lock" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/crates/oo-gateway/src/bin/query-pg.rs" ]; then
  echo "archive missing query-pg" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -e "$ROOT/archive/benchmarks/sql-runner" ]; then
  echo "archive already contains sql-runner" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
cp -a "$ROOT/archive/." "$ROOT/proof-src/"
mv "$ROOT/proof-src/rust-toolchain.toml" "$ROOT/proof-src/rust-toolchain.toml.repo"
rm -rf "$ROOT/proof-src/benchmarks/sql-runner"
cp -a "$SRC/benchmarks/sql-runner" "$ROOT/proof-src/benchmarks/sql-runner"
rm -rf "$ROOT/proof-src/benchmarks/sql-runner/target"
find "$ROOT/proof-src/benchmarks/sql-runner" -name '.DS_Store' -delete
if [ ! -f "$ROOT/proof-src/benchmarks/sql-runner/Cargo.lock" ]; then
  echo "overlay is missing Cargo.lock" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -e "$ROOT/archive/benchmarks/sql-runner" ]; then
  echo "overlay leaked into the immutable archive" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
shasum -a 256 "$ROOT/archive/rust-toolchain.toml" > "$EVIDENCE/toolchain-file.sha256"
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"

own=$(docker ps -q --filter label=oo.owner=a231 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10 cap_own=1\n' "$own" "$global_builders" "$host_cargo" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: rust builder capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt" || exit 2
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi

admin_file() {
  local dest="$1"
  local sql="$2"
  local pid
  local ticks=0
  docker exec "$PGNAME" /fixture/psql-admin.sh -d oo_a198 -v ON_ERROR_STOP=1 -f "/fixture/${sql}" \
    > "${dest}.stdout" 2> "${dest}.stderr" &
  pid=$!
  while kill -0 "$pid" 2>/dev/null && [ "$ticks" -lt 40 ]; do
    sleep 1
    ticks=$((ticks + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    printf '%s\n' 124 > "${dest}.exit"
    return 0
  fi
  wait "$pid"
  printf '%s\n' "$?" > "${dest}.exit"
  return 0
}

printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  --mount "type=bind,source=${ROOT}/proof-src,target=/src" \
  --mount "type=bind,source=${ROOT}/archive,target=/archive,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE}/extract-libclang.sh,target=/extract-libclang.sh,readonly" \
  --mount "type=bind,source=${TOOLS},target=/tools,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/run/oo-a198/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
  --tmpfs /tight:rw,nosuid,nodev,noexec,size=64k \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a231 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a231 \
  --label oo.task=sql-runner-proof \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1 &
run_pid=$!
printf '%s\n' "$run_pid" > "$EVIDENCE/docker-pid.txt"
tick=0
while kill -0 "$run_pid" 2>/dev/null; do
  if [ -f "$EVIDENCE/hold-needed" ] && [ ! -f "$EVIDENCE/hold-ready" ] && [ ! -f "$EVIDENCE/admin/hold.exit" ]; then
    printf '%s hold\n' "$(date -u +%H:%M:%S)" >> "$EVIDENCE/host-coord.log"
    admin_file "$EVIDENCE/admin/hold" hold.sql
    admin_file "$EVIDENCE/admin/observe" observe.sql
    admin_file "$EVIDENCE/admin/backends-after-hold" backends.sql
    docker exec "$PGNAME" cat /tmp/oo-a231-auth-mode > "$EVIDENCE/admin/auth-mode.txt" 2>/dev/null || true
    date -u +%Y-%m-%dT%H:%M:%SZ > "$EVIDENCE/hold-ready"
  fi
  if [ -f "$EVIDENCE/timeout-needed" ] && [ ! -f "$EVIDENCE/admin/timeout-started" ]; then
    printf '%s timeout-start\n' "$(date -u +%H:%M:%S)" >> "$EVIDENCE/host-coord.log"
    date -u +%Y-%m-%dT%H:%M:%SZ > "$EVIDENCE/admin/timeout-started"
    docker exec "$PGNAME" /fixture/lock-hold.sh > "$EVIDENCE/admin/lock.stdout" 2> "$EVIDENCE/admin/lock.stderr" &
    printf '%s\n' "$!" > "$EVIDENCE/admin/lock.pid"
  fi
  if [ -f "$EVIDENCE/admin/timeout-started" ] && [ ! -f "$EVIDENCE/timeout-ready" ] && [ ! -f "$EVIDENCE/admin/timeout-failed" ]; then
    if docker exec "$PGNAME" test -f /tmp/oo-a231-locked >/dev/null 2>&1; then
      docker exec "$PGNAME" cat /tmp/oo-a231-physical-name > "$EVIDENCE/admin/physical-name.txt"
      admin_file "$EVIDENCE/admin/backends-during-lock" backends.sql
      printf '%s timeout-ready\n' "$(date -u +%H:%M:%S)" >> "$EVIDENCE/host-coord.log"
      date -u +%Y-%m-%dT%H:%M:%SZ > "$EVIDENCE/timeout-ready"
    elif [ -f "$EVIDENCE/admin/lock.pid" ] && ! kill -0 "$(cat "$EVIDENCE/admin/lock.pid")" 2>/dev/null; then
      printf '%s timeout-failed\n' "$(date -u +%H:%M:%S)" >> "$EVIDENCE/host-coord.log"
      date -u +%Y-%m-%dT%H:%M:%SZ > "$EVIDENCE/admin/timeout-failed"
    fi
  fi
  if [ -f "$EVIDENCE/timeout-release" ] && [ ! -f "$EVIDENCE/timeout-released" ]; then
    printf '%s timeout-release\n' "$(date -u +%H:%M:%S)" >> "$EVIDENCE/host-coord.log"
    if [ -f "$EVIDENCE/admin/lock.pid" ]; then
      kill "$(cat "$EVIDENCE/admin/lock.pid")" 2>/dev/null || true
    fi
    admin_file "$EVIDENCE/admin/release" release.sql
    admin_file "$EVIDENCE/admin/backends-after-release" backends.sql
    admin_file "$EVIDENCE/admin/sweep" sweep.sql
    admin_file "$EVIDENCE/admin/backends-after-sweep" backends.sql
    date -u +%Y-%m-%dT%H:%M:%SZ > "$EVIDENCE/timeout-released"
  fi
  if [ -f "$EVIDENCE/counts-needed" ] && [ ! -f "$EVIDENCE/counts-ready" ]; then
    printf '%s counts\n' "$(date -u +%H:%M:%S)" >> "$EVIDENCE/host-coord.log"
    admin_file "$EVIDENCE/admin/backends-final" backends.sql
    date -u +%Y-%m-%dT%H:%M:%SZ > "$EVIDENCE/counts-ready"
  fi
  tick=$((tick + 1))
  if [ $((tick % 60)) -eq 0 ]; then
    recent=$(ls -t "$EVIDENCE/phases" 2>/dev/null | head -1 || true)
    printf '%s tick=%s phase=%s\n' "$(date -u +%H:%M:%S)" "$tick" "${recent:-none}" >> "$EVIDENCE/host-coord.log"
  fi
  sleep 1
done
wait "$run_pid"
run_exit=$?
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
exit "$run_exit"
