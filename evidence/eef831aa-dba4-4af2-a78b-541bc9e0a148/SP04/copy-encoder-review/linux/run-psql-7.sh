#!/bin/bash
# PostgreSQL decode of the attempt-6 COPY bytes. No second Rust builder.
# docker cp into the container tmpfs is hidden from the server, so bytes go through docker exec.
set -euo pipefail

SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a148/SP04/copy-encoder-review
PREV=${EVIDENCE}/linux/oracle-attempt-6
SQL=${EVIDENCE}/linux/oracle
ATTEMPT=${EVIDENCE}/linux/oracle-attempt-7
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
PG_NAME=oo-a148-pg
NET=oo-a148-eef831aa-net
CARGO_VOL=oo-a148-eef831aa-cargo
TARGET_VOL=oo-a148-eef831aa-target
PG_VOL=oo-a148-eef831aa-pg
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a148
success=0
created_pg_vol=0

mkdir -p "$ATTEMPT"
if [ -f "$ATTEMPT/psql-main.stdout" ]; then
  echo "refusing to overwrite attempt 7 psql output" >&2
  exit 2
fi
printf '%s\n' "psql_only source_bytes=oracle-attempt-6 rust_not_repeated=1 reason=docker_cp_misses_tmpfs" > "$ATTEMPT/why.txt"
test "$(cat "$PREV/test.exit")" = "0"
test "$(cat "$PREV/clippy.exit")" = "0"
test -s "$PREV/fresh.copy"
test -s "$PREV/domain-violation.copy"

cleanup() {
  {
    echo "cleanup_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "success=${success}"
    docker inspect "$PG_NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} memory={{.HostConfig.Memory}}' \
      > "$ATTEMPT/postgres-hostconfig.txt" 2>/dev/null || true
    docker logs "$PG_NAME" > "$ATTEMPT/postgres.log" 2>&1 || true
    echo "rm_containers:"
    docker rm -f "$PG_NAME" 2>&1 || true
    if [ "$success" -eq 1 ]; then
      echo "rm_volumes:"
      docker volume rm "$CARGO_VOL" 2>&1 || true
      docker volume rm "$TARGET_VOL" 2>&1 || true
      docker volume rm "$PG_VOL" 2>&1 || true
    else
      echo "volumes_kept_for_rerun=cargo,target"
      if [ "$created_pg_vol" -eq 1 ]; then
        docker volume rm "$PG_VOL" 2>&1 || true
      fi
    fi
    echo "rm_network:"
    docker network rm "$NET" 2>&1 || true
    echo "remaining_a148:"
    docker ps -a --filter label=oo.owner=a148 --format 'name={{.Names}} status={{.Status}}' 2>&1 || true
    docker volume ls --filter label=oo.owner=a148 --format '{{.Name}}' 2>&1 || true
    docker network ls --filter label=oo.owner=a148 --format '{{.Name}}' 2>&1 || true
  } > "$ATTEMPT/cleanup.txt" 2>&1 || true
  if [ "$success" -eq 1 ]; then
    printf '%s\n' "DONE" > "$ATTEMPT/run-status.txt"
  elif [ ! -f "$ATTEMPT/run-status.txt" ]; then
    printf '%s\n' "FAILED" > "$ATTEMPT/run-status.txt"
  fi
}
trap cleanup EXIT

running_rust=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
own=$(docker ps -q --filter label=oo.owner=a148 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
printf 'running_rust_builders=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$ATTEMPT/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  printf '%s\n' "FAILED capacity" > "$ATTEMPT/run-status.txt"
  exit 3
fi

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology --label oo.owner=a148 \
    --label oo.session="$SESSION" --label oo.kind=network \
    "$NET" >/dev/null
fi
if docker volume inspect "$PG_VOL" >/dev/null 2>&1; then
  printf '%s\n' "FAILED pg_volume" > "$ATTEMPT/run-status.txt"
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology --label oo.owner=a148 \
  --label oo.session="$SESSION" --label oo.kind=postgres \
  "$PG_VOL" >/dev/null
created_pg_vol=1

docker run -d \
  --name "$PG_NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --cap-add CHOWN --cap-add DAC_OVERRIDE --cap-add FOWNER --cap-add SETGID --cap-add SETUID \
  --memory 1g --memory-swap 1g --cpus 1 --pids-limit 256 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,nodev,size=256m \
  --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  --label oo.project=open-ontology \
  --label oo.owner=a148 \
  --label oo.session="$SESSION" \
  --label oo.task=copy-encoder-psql-7 \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  "$PG_IMAGE" \
  -c listen_addresses='*' -c max_connections=40 -c shared_preload_libraries= \
  > "$ATTEMPT/postgres-start.txt"

ready=0
for _ in $(seq 1 60); do
  if docker exec "$PG_NAME" pg_isready -U postgres -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
printf 'ready=%s\n' "$ready" > "$ATTEMPT/postgres-ready.txt"
if [ "$ready" -ne 1 ]; then
  printf '%s\n' "FAILED postgres_not_ready" > "$ATTEMPT/run-status.txt"
  exit 1
fi

put() {
  docker exec -i -u postgres "$PG_NAME" sh -c "cat > '$2'" < "$1"
}
put "$PREV/fresh.copy" /tmp/fresh.copy
put "$PREV/domain-violation.copy" /tmp/domain-violation.copy
put "$SQL/load.sql" /tmp/load.sql
put "$SQL/domain.sql" /tmp/domain.sql
docker exec -u postgres "$PG_NAME" ls -l /tmp/fresh.copy /tmp/domain-violation.copy /tmp/load.sql /tmp/domain.sql \
  > "$ATTEMPT/container-inputs.txt"

set +e
docker exec -u postgres "$PG_NAME" psql -U postgres -d postgres -f /tmp/load.sql \
  > "$ATTEMPT/psql-main.stdout" 2> "$ATTEMPT/psql-main.stderr"
psql_main=$?
set -e
printf '%s\n' "$psql_main" > "$ATTEMPT/psql-main.exit"
if [ "$psql_main" -ne 0 ]; then
  printf '%s\n' "FAILED psql_main=${psql_main}" > "$ATTEMPT/run-status.txt"
  exit "$psql_main"
fi

take() {
  docker exec -u postgres "$PG_NAME" cat "$1" > "$2"
}
take /tmp/a148-version.txt "$ATTEMPT/pg-version.txt"
take /tmp/a148-count.txt "$ATTEMPT/pg-count.txt"
take /tmp/a148-schema.txt "$ATTEMPT/pg-schema.txt"
take /tmp/a148-rows.txt "$ATTEMPT/pg-rows.txt"
take /tmp/a148-rollback.txt "$ATTEMPT/pg-rollback.txt"

set +e
diff -u "$PREV/expected-schema.txt" "$ATTEMPT/pg-schema.txt" > "$ATTEMPT/schema.diff"
schema_diff=$?
diff -u "$PREV/expected-rows.txt" "$ATTEMPT/pg-rows.txt" > "$ATTEMPT/rows.diff"
rows_diff=$?
set -e
printf '%s\n' "$schema_diff" > "$ATTEMPT/schema-diff.exit"
printf '%s\n' "$rows_diff" > "$ATTEMPT/rows-diff.exit"
grep -q '17.10' "$ATTEMPT/pg-version.txt"
grep -q '170010' "$ATTEMPT/pg-version.txt"
grep -qx '2' "$ATTEMPT/pg-count.txt"
grep -qx 'null' "$ATTEMPT/pg-rollback.txt"
if [ "$schema_diff" -ne 0 ] || [ "$rows_diff" -ne 0 ]; then
  printf '%s\n' "FAILED compare schema=${schema_diff} rows=${rows_diff}" > "$ATTEMPT/run-status.txt"
  exit 1
fi

set +e
docker exec -u postgres "$PG_NAME" psql -U postgres -d postgres -f /tmp/domain.sql \
  > "$ATTEMPT/psql-domain.stdout" 2> "$ATTEMPT/psql-domain.stderr"
psql_domain=$?
set -e
printf '%s\n' "$psql_domain" > "$ATTEMPT/psql-domain.exit"
if [ "$psql_domain" -eq 0 ]; then
  printf '%s\n' "FAILED domain_accepted" > "$ATTEMPT/run-status.txt"
  exit 1
fi
if ! grep -q '23514' "$ATTEMPT/psql-domain.stderr"; then
  printf '%s\n' "FAILED domain_sqlstate" > "$ATTEMPT/run-status.txt"
  exit 1
fi

docker exec -u postgres "$PG_NAME" psql -U postgres -d postgres -v ON_ERROR_STOP=1 -At -F '|' -c \
  "SELECT CASE WHEN to_regclass('oo_copy_a148.fresh') IS NULL THEN 'null' ELSE 'present' END, CASE WHEN to_regclass('oo_copy_a148_chk.checked') IS NULL THEN 'null' ELSE 'present' END, CASE WHEN to_regclass('oo_copy_a148_chk.pos_int') IS NULL THEN 'null' ELSE 'present' END;" \
  > "$ATTEMPT/pg-after.txt"
grep -Fxq 'null|null|null' "$ATTEMPT/pg-after.txt"

success=1
printf '%s\n' "DONE" > "$ATTEMPT/run-status.txt"
exit 0
