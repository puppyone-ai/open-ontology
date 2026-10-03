#!/bin/bash
# One owned Rust 1.94 builder for the evidence-only prerequisite. PostgreSQL is already running.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a125
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
REPO=/Users/redux/puppy-one/open-ontology
LIVE=${REPO}/evidence/${RUN_ID}/SP03/current-schema-guard/prereq
OUT=${OO_A125_OUT:-${LIVE}/out}
ARCH=/tmp/oo-a125-${RUN_ID}/archive
SEC=/tmp/oo-a125-${RUN_ID}/secrets
NAME=${RUN_ID}-rust-prereq${OO_A125_NAME_SUFFIX:-}
PG=${RUN_ID}-pg
NET=${RUN_ID}-net
CARGO_VOL=${RUN_ID}-cargo
TARGET_VOL=${RUN_ID}-target
PGLOG=${RUN_ID}-pglog

test -f "$ARCH/Cargo.toml"
test -f "$SEC/pointer.json"
test -f "${LIVE}/a125_prereq.rs"
docker image inspect "$IMAGE" >/dev/null
docker network inspect "$NET" >/dev/null
if ! docker ps --filter "name=^/${PG}$" --filter status=running --format '{{.ID}}' | grep -q .; then
  echo "postgres is not running" >&2
  exit 1
fi

mkdir -p "$OUT"
ps -axo pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|rustup|clippy-driver)$/ {print}' > "${OUT}/host-cargo-procs.txt" || true
host_n=$(grep -c . "${OUT}/host-cargo-procs.txt" || true)
docker ps --filter label=oo.kind=rust-builder --format '{{.ID}} {{.Names}} {{.Status}}' > "${OUT}/builders-before.txt" || true
builder_n=$(grep -c . "${OUT}/builders-before.txt" || true)
global_n=$((host_n + builder_n))
{
  echo "captured_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host_cargo=$host_n"
  echo "rust_builders=$builder_n"
  echo "global=$global_n"
  echo "own_take=1"
} | tee "${OUT}/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a125 --filter label=oo.kind=rust-builder --format '{{.ID}}' || true)
if [ -n "$own" ]; then
  echo "deferred: this session already has a running rust builder" >&2
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.ID}}' | grep -q .; then
  state=$(docker inspect -f '{{.State.Status}}' "$NAME")
  echo "refusing to replace existing ${NAME} status=${state}" >&2
  exit 1
fi

chmod 755 "${LIVE}/inside.sh" "${LIVE}/launch.sh"
echo "prereq_start=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee "${OUT}/started.txt"
set +e
docker run \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 8g \
  --memory-swap 8g \
  --cpus 4 \
  --pids-limit 4096 \
  --network "$NET" \
  --workdir /tmp \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e OO_A125_TEST_FILE=${OO_A125_TEST_FILE:-a125_prereq.rs} \
  -e OO_A125_TEST_BIN=${OO_A125_TEST_BIN:-a125_prereq} \
  -e OO_A125_PREREQ_POINTER=/secrets/pointer.json \
  -e OO_A125_PREREQ_OUTCOME=/evidence/outcome.txt \
  -e OO_A125_PREREQ_SQL=/evidence/sql-by-actor.txt \
  --mount "type=bind,source=${ARCH},target=/src,readonly" \
  --mount "type=bind,source=${LIVE},target=/prereq,readonly" \
  --mount "type=bind,source=${LIVE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${OUT},target=/evidence" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a125 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=current-schema-guard \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp03-current-schema-prereq \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "${OUT}/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "${OUT}/docker-run.exit"
echo "prereq_end=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "${OUT}/started.txt"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "${OUT}/inspect-safe.txt" || true
docker cp "$NAME":/target/phases/prereq.exit "${OUT}/prereq.exit" >/dev/null 2>&1 || true

leak=0
if docker exec -u postgres "$PG" test -f /pglog/postgresql.log; then
  for file in "$SEC"/super.pass "$SEC"/reader.pass "$SEC"/writer.pass "$SEC"/guc.pass "$SEC"/lot-reader.pass "$SEC"/lot-writer.pass; do
    if docker exec -u postgres "$PG" grep -F -q -f "/secrets/$(basename "$file")" /pglog/postgresql.log; then
      leak=1
    fi
  done
fi
printf '%s\n' "$leak" > "${OUT}/log-secret-hits.txt"
if [ "$leak" -eq 0 ]; then
  docker cp "$PG":/pglog/postgresql.log "${OUT}/postgresql.log" >/dev/null
fi
for produced in "${OUT}/outcome.txt" "${OUT}/sql-by-actor.txt" "${OUT}/console.log"; do
  if [ -f "$produced" ]; then
    for file in "$SEC"/super.pass "$SEC"/reader.pass "$SEC"/writer.pass "$SEC"/guc.pass "$SEC"/lot-reader.pass "$SEC"/lot-writer.pass; do
      if grep -F -q -f "$file" "$produced"; then
        echo "evidence_leak $(basename "$produced")" >> "${OUT}/log-secret-hits.txt"
        rm -f "$produced"
      fi
    done
  fi
done
exit "$run_exit"
