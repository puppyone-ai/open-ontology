#!/bin/bash
# One sequential Rust 1.94 oracle against the owned PostgreSQL 17.10 fixture.
# The full run and the restart run never overlap, and neither overlaps another rust builder.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a107
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP02/lease-operations-review
NAME=${RUN_ID}-oracle
RESTART_NAME=${RUN_ID}-oracle-restart
PG=${RUN_ID}-pg
NET=${RUN_ID}-net
CARGO_VOL=${RUN_ID}-oracle-cargo
TARGET_VOL=${RUN_ID}-oracle-target
OUT=${EVIDENCE}/oracle/out
SRC=${EVIDENCE}/archive/snapshot

ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "$EVIDENCE/oracle/host-cargo-procs.txt" || true
host_n=$(grep -c . "$EVIDENCE/oracle/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.ID}} {{.Names}}' > "$EVIDENCE/oracle/builders-before.txt" || true
builder_n=$(grep -c . "$EVIDENCE/oracle/builders-before.txt" || true)
global_n=$((host_n + builder_n))
echo "host_cargo=$host_n rust_builders=$builder_n global=$global_n own_take=1 postgres_not_counted_as_rust=1 empty_does_not_grant_second=1" | tee "$EVIDENCE/oracle/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a107 --filter label=oo.kind=rust-builder --format '{{.ID}}' || true)
if [ -n "$own" ]; then
  echo "deferred: this task already has a running rust builder" >&2
  exit 3
fi

pg_state=$(docker inspect "$PG" --format '{{.State.Status}}' 2>/dev/null || true)
if [ "$pg_state" = "exited" ]; then
  docker start "$PG" >/dev/null
  pg_state=running
fi
if [ "$pg_state" != "running" ]; then
  echo "owned postgres is not running: ${pg_state:-absent}" >&2
  exit 4
fi
pids_before=$(docker inspect "$PG" --format '{{.HostConfig.PidsLimit}}')
docker update --pids-limit 1024 "$PG" >/dev/null
pids_after=$(docker inspect "$PG" --format '{{.HostConfig.PidsLimit}}')
printf 'before=%s after=%s\n' "$pids_before" "$pids_after" > "$EVIDENCE/oracle/pg-pids.txt"

mkdir -p "$OUT"
rm -rf "$OUT"
mkdir -p "$OUT"
chmod 755 "$EVIDENCE/oracle/inside.sh"
(
  cd "$EVIDENCE/oracle"
  shasum -a 256 Cargo.toml Cargo.lock src/main.rs src/pg.rs src/sha256.rs inside.sh
) > "$EVIDENCE/oracle/source.sha256"

docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a107 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a107 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=rust-builder \
  "$TARGET_VOL" >/dev/null

for old in "$NAME" "$RESTART_NAME"; do
  st=$(docker inspect "$old" --format '{{.State.Status}}' 2>/dev/null || true)
  if [ "$st" = "exited" ] || [ "$st" = "created" ]; then
    docker rm "$old" >/dev/null
  elif [ -n "$st" ]; then
    echo "refusing to replace running container $old" >&2
    exit 3
  fi
done

cat > "$EVIDENCE/oracle/invocation.txt" <<EOF
run_id=${RUN_ID}
kind=independent-login-oracle
image=${IMAGE}
platform=linux/arm64
toolchain=1.94.0
msrv_override=RUSTUP_TOOLCHAIN=1.94.0
network=${NET}
container=${NAME}
restart_container=${RESTART_NAME}
postgres=${PG}
pghost=${PG}
pgport=5432
auth=trust_fixture_only
password_on_argv=no
sqldir=/src/crates/oo-jobs/migrations
srcdir=/src
outdir=/out
snapshot=${SRC}
privileged=no
network_mode=bridge_not_host
home_mount=no
socket_mount=no
host_port=none
shared_cargo_cache=no
pull=never
cap_drop=ALL
cap_add=none
init=yes
no_new_privileges=yes
author_container=not_used
author_container_name_observed_in_source=oo-a102-pg1710-jobs
EOF

common_run=(
  --platform linux/arm64
  --pull never
  --init
  --user 0:0
  --security-opt no-new-privileges
  --cap-drop ALL
  --memory 4g
  --cpus 2
  --pids-limit 4096
  --network "$NET"
  --workdir /tmp
  --tmpfs /tmp:rw,nosuid,nodev,size=1g
  -e CARGO_TERM_COLOR=never
  -e CARGO_HOME=/cargo-home
  -e CARGO_TARGET_DIR=/target
  -e RUSTUP_TOOLCHAIN=1.94.0
  -e HOME=/tmp
  -e PGHOST="$PG"
  -e PGPORT=5432
  -e SQLDIR=/src/crates/oo-jobs/migrations
  -e SRCDIR=/src
  -e OUTDIR=/out
  --mount "type=bind,source=${SRC},target=/src,readonly"
  --mount "type=bind,source=${EVIDENCE}/oracle,target=/oracle-src,readonly"
  --mount "type=bind,source=${OUT},target=/out"
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home"
  --mount "type=volume,source=${TARGET_VOL},target=/target"
  --label oo.project=open-ontology
  --label oo.owner=a107
  --label oo.session="${RUN_ID}"
  --label oo.run_id="${RUN_ID}"
  --label oo.scope=sp02-w01-b1
  --label oo.toolchain=1.94.0
  --label oo.platform=linux/arm64
  --label oo.kind=rust-builder
)

set +e
docker run --name "$NAME" "${common_run[@]}" \
  --label oo.task=lease-operations-review-oracle \
  "$IMAGE" \
  bash /oracle-src/inside.sh > "$EVIDENCE/oracle/console.log" 2>&1 &
run_pid=$!
ready=0
for _ in $(seq 1 360); do
  if [ -f "$OUT/hold-ready.txt" ]; then
    ready=1
    break
  fi
  if ! kill -0 "$run_pid" 2>/dev/null; then
    break
  fi
  sleep 1
done
if [ "$ready" = 1 ]; then
  date -u +%Y-%m-%dT%H:%M:%SZ > "$EVIDENCE/oracle/restart-issued.txt"
  docker restart "$PG" >> "$EVIDENCE/oracle/restart-issued.txt" 2>&1
fi
wait "$run_pid"
full_exit=$?
set -e
printf '%s\n' "$full_exit" > "$EVIDENCE/oracle/docker-run.exit"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} pid={{.State.Pid}}' > "$EVIDENCE/oracle/inspect-safe.txt" || true
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "$EVIDENCE/oracle/cap-drop.json" || true
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/oracle/ports.json" || true
docker inspect "$NAME" --format '{{range .Config.Env}}{{println .}}{{end}}' | awk -F= '{print $1}' | sort > "$EVIDENCE/oracle/env-keys.txt" || true

if [ ! -f "$OUT/marker-queued.txt" ] || [ ! -f "$OUT/marker-running.txt" ] || [ ! -f "$OUT/marker-open.txt" ]; then
  echo "restart phase not run: durability markers missing" | tee "$EVIDENCE/oracle/restart-skipped.txt"
  printf '%s\n' "skipped" > "$EVIDENCE/oracle/restart.exit"
  exit "$full_exit"
fi

ready_pg=0
for _ in $(seq 1 60); do
  if docker exec "$PG" pg_isready -h 127.0.0.1 -U postgres -d postgres >/dev/null 2>&1; then
    ready_pg=1
    break
  fi
  sleep 1
done
if [ "$ready_pg" != 1 ]; then
  echo "postgres did not become ready after restart" | tee "$EVIDENCE/oracle/restart-skipped.txt"
  printf '%s\n' "pg_not_ready" > "$EVIDENCE/oracle/restart.exit"
  exit 4
fi

set +e
docker run --name "$RESTART_NAME" "${common_run[@]}" \
  --label oo.task=lease-operations-review-oracle-restart \
  "$IMAGE" \
  /target/debug/lease_oracle restart > "$EVIDENCE/oracle/restart-console.log" 2>&1
restart_exit=$?
set -e
printf '%s\n' "$restart_exit" > "$EVIDENCE/oracle/restart.exit"
docker inspect "$RESTART_NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$EVIDENCE/oracle/restart-inspect-safe.txt" || true
if [ "$full_exit" -ne 0 ]; then
  exit "$full_exit"
fi
exit "$restart_exit"
