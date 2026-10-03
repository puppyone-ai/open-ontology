#!/bin/bash
# One owned Rust 1.94 builder after the pre-edit smoke has exited.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a128
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
REPO=/Users/redux/puppy-one/open-ontology
LIVE=${REPO}/evidence/${RUN_ID}/SP03/catalog-rate/verify
OUT=${LIVE}/out
SEC=/tmp/oo-a128-${RUN_ID}/secrets
NAME=${RUN_ID}-rust-verify
PG=${RUN_ID}-pg
NET=${RUN_ID}-net
CARGO_VOL=${RUN_ID}-cargo
TARGET_VOL=${RUN_ID}-target
PGLOG=${RUN_ID}-pglog

test -f "${REPO}/Cargo.toml"
test -f "${LIVE}/inside.sh"
test -f "$SEC/pointer.json"
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
  echo "source=working-tree"
} | tee "${OUT}/capacity.txt"
if [ "$global_n" -ge 2 ]; then
  echo "deferred: global rust builder capacity is full" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a128 --filter label=oo.kind=rust-builder --format '{{.ID}}' || true)
if [ -n "$own" ]; then
  echo "deferred: this session already has a running rust builder" >&2
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.ID}}' | grep -q .; then
  state=$(docker inspect -f '{{.State.Status}}' "$NAME")
  echo "refusing to replace existing ${NAME} status=${state}" >&2
  exit 1
fi

echo "log_rotate_start=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee "${OUT}/log-rotate.txt"
docker exec -u postgres "$PG" sh -c ': > /pglog/postgresql.log'
docker exec -u postgres "$PG" psql -d snapdb -v ON_ERROR_STOP=1 -At -c "SELECT pg_catalog.pg_rotate_logfile()" >> "${OUT}/log-rotate.txt"
docker exec -u postgres "$PG" sh -c ': > /pglog/postgresql.log; sleep 1; : > /pglog/postgresql.log'
docker exec -u postgres "$PG" psql -d snapdb -v ON_ERROR_STOP=1 -At -c "SELECT pg_catalog.pg_rotate_logfile()" >> "${OUT}/log-rotate.txt"
docker exec -u postgres "$PG" psql -d snapdb -v ON_ERROR_STOP=1 -At -c "SELECT 1" >> "${OUT}/log-rotate.txt"
first=$(docker exec -u postgres "$PG" od -An -tx1 -N 1 /pglog/postgresql.log | tr -d ' \n')
echo "log_first_byte=${first}" | tee -a "${OUT}/log-rotate.txt"
if [ -z "$first" ] || [ "$first" = "00" ]; then
  echo "log rotate produced an empty file or a leading NUL" >&2
  exit 1
fi

chmod 755 "${LIVE}/inside.sh" "${LIVE}/launch.sh"
echo "verify_start=$(date -u +%Y-%m-%dT%H:%M:%SZ)" | tee "${OUT}/started.txt"
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
  --workdir /src \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=4 \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e OO_PG_CATALOG_RATE_FIXTURE=/secrets/pointer.json \
  -e OO_PG_SNAPSHOT_FIXTURE=/secrets/pointer.json \
  -e OO_PG_SNAPSHOT_OUTCOME=/evidence/outcome.txt \
  --mount "type=bind,source=${REPO},target=/src,readonly" \
  --mount "type=bind,source=${LIVE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${OUT},target=/evidence" \
  --mount "type=volume,source=${PGLOG},target=/pglog,readonly" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a128 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=catalog-rate \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp03-catalog-rate-verify \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "${OUT}/console.log" 2>&1
run_exit=$?
set -e
printf '%s\n' "$run_exit" > "${OUT}/docker-run.exit"
echo "verify_end=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "${OUT}/started.txt"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "${OUT}/inspect-safe.txt" || true
mkdir -p "${OUT}/phases"
docker cp "$NAME":/target/phases/. "${OUT}/phases" || true

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
for produced in "${OUT}/outcome.txt" "${OUT}/console.log" "${OUT}/postgresql.log"; do
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
