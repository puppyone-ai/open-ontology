#!/bin/bash
# Oracle attempt 2. Smoke already passed and is not repeated.
# Harness-only fixes: position temporaries and hex digit chars.
set -euo pipefail

SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a148/SP04/copy-encoder-review
SMOKE_EV=${EVIDENCE}/linux/smoke
ORACLE_SRC=${EVIDENCE}/linux/oracle
ATTEMPT=${EVIDENCE}/linux/oracle-attempt-5
IDENT=${EVIDENCE}/identity
ROOT=/tmp/oo-a148-eef831aa-dba4-4af2-a78b-541bc9e0a148-a5
SNAP=${ROOT}/oracle-src
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
ORACLE_NAME=oo-a148-oracle
PG_NAME=oo-a148-pg
NET=oo-a148-eef831aa-net
CARGO_VOL=oo-a148-eef831aa-cargo
TARGET_VOL=oo-a148-eef831aa-target
PG_VOL=oo-a148-eef831aa-pg
FIXED=d80472d8a99afd66d8d0e71420f7be2bb62e65c8
EXPECT_TREE=a3b10827acfac28c4abf986831cbcb26823477dc
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a148
success=0
created_pg_vol=0

mkdir -p "$ATTEMPT" "$ROOT"
if [ -f "$ATTEMPT/console.log" ]; then
  echo "refusing to overwrite oracle attempt 2 console.log" >&2
  exit 2
fi
printf '%s\n' "harness_rerun_after_compile smoke_not_repeated=1" > "$ATTEMPT/why.txt"

cleanup() {
  {
    echo "cleanup_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "success=${success}"
    docker inspect "$ORACLE_NAME" --format 'ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' \
      > "$ATTEMPT/inspect.txt" 2>/dev/null || true
    docker inspect "$ORACLE_NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} memory={{.HostConfig.Memory}}' \
      > "$ATTEMPT/hostconfig.txt" 2>/dev/null || true
    docker inspect "$PG_NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} memory={{.HostConfig.Memory}}' \
      > "$ATTEMPT/postgres-hostconfig.txt" 2>/dev/null || true
    docker logs "$PG_NAME" > "$ATTEMPT/postgres.log" 2>&1 || true
    echo "rm_containers:"
    docker rm -f "$ORACLE_NAME" "$PG_NAME" 2>&1 || true
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
    rm -rf "$ROOT"
    echo "snapshot_removed=1"
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

file_sha() { shasum -a 256 "$1" | awk '{print $1}'; }

running_rust=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
own=$(docker ps -q --filter label=oo.owner=a148 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
{
  echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}'
  echo "host_toolchain_comm:"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver|rustup)$/ {print}' || true
} > "$ATTEMPT/builders-before.txt"
printf 'running_rust_builders=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "$ATTEMPT/capacity.txt"
if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
  echo "deferred: rust builder capacity is full" >&2
  printf '%s\n' "FAILED capacity" > "$ATTEMPT/run-status.txt"
  exit 3
fi

test "$(git -C "$SRC" rev-parse "${FIXED}^{tree}")" = "$EXPECT_TREE"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology --label oo.owner=a148 \
    --label oo.session="$SESSION" --label oo.kind=network \
    "$NET" >/dev/null
fi
for vol in "$CARGO_VOL" "$TARGET_VOL"; do
  owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$vol")
  test "$owner" = "a148"
done
if docker volume inspect "$PG_VOL" >/dev/null 2>&1; then
  echo "refusing a pre-existing postgres volume" >&2
  printf '%s\n' "FAILED pg_volume" > "$ATTEMPT/run-status.txt"
  exit 2
fi

rm -rf "$SNAP"
mkdir -p "$SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SNAP"
cp "$ORACLE_SRC/a148_copy_oracle.rs" "$SNAP/crates/oo-pipeline/tests/a148_copy_oracle.rs"
for rel in \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/copy_encoder.rs \
  crates/oo-pipeline/tests/copy_encoder_live.rs \
  crates/oo-pipeline/src/physical_mapping.rs \
  crates/oo-pipeline/Cargo.toml \
  contracts/pipeline/copy-encoder-v1.md \
  contracts/source/pgsql/type-conversion-v1.md
do
  test "$(git -C "$SRC" hash-object "$SNAP/$rel")" = "$(git -C "$SRC" rev-parse "${FIXED}:${rel}")"
done
cp "$SMOKE_EV"/expect-lib.sha256 "$SMOKE_EV"/expect-encoder.sha256 "$SMOKE_EV"/expect-live.sha256 \
  "$SMOKE_EV"/expect-mapping.sha256 "$SMOKE_EV"/expect-cargo.sha256 "$SMOKE_EV"/expect-contract.sha256 \
  "$SMOKE_EV"/expect-types.sha256 "$ATTEMPT"/
file_sha "$SNAP/crates/oo-pipeline/tests/a148_copy_oracle.rs" > "$ATTEMPT/expect-overlay.sha256"

cat > "$ATTEMPT/invocation.txt" <<EOF
source_commit=${FIXED}
tree=${EXPECT_TREE}
attempt=5
smoke_repeated=0
oracle_composition=git archive of ${FIXED} plus tests/a148_copy_oracle.rs
rust_image=${RUST_IMAGE}
postgres_image=${PG_IMAGE}
platform=linux/arm64
toolchain=1.94.0
pull=never
privileged=no
home_mount=no
socket_mount=no
host_network=no
shared_cargo_cache=no
published_ports=no
auth=trust
rust_caps=drop_all
postgres_caps=drop_all plus CHOWN DAC_OVERRIDE FOWNER SETGID SETUID
phases=oracle_test oracle_clippy psql_decode domain_check
EOF

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
  --label oo.task=copy-encoder-oracle-2 \
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

if [ ! -f "$SNAP/Cargo.toml" ]; then
  echo "host snapshot missing before docker run" > "$ATTEMPT/snap-host.txt"
  printf '%s\n' "FAILED host_snapshot_missing" > "$ATTEMPT/run-status.txt"
  exit 2
fi
{
  echo "host_snap=$SNAP"
  ls -ld "$SNAP"
  wc -c "$SNAP/Cargo.toml"
} > "$ATTEMPT/snap-host.txt"
set +e
docker run --rm \
  --platform linux/arm64 \
  --pull never \
  --entrypoint /bin/ls \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --network none \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a148 \
  --label oo.session="$SESSION" \
  --label oo.kind=rust-builder \
  --label oo.task=copy-encoder-snapcheck \
  "$RUST_IMAGE" \
  /src/Cargo.toml > "$ATTEMPT/snap-docker.txt" 2>&1
printf '%s\n' "$?" > "$ATTEMPT/snap-docker.exit"
set -e
if [ "$(cat "$ATTEMPT/snap-docker.exit")" -ne 0 ]; then
  printf '%s\n' "FAILED snap_docker_mount" > "$ATTEMPT/run-status.txt"
  exit 1
fi

set +e
docker run \
  --name "$ORACLE_NAME" \
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
  -e OO_A148_EVIDENCE=/evidence \
  --mount "type=bind,source=${SNAP},target=/src,readonly" \
  --mount "type=bind,source=${ORACLE_SRC}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${ATTEMPT},target=/evidence" \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a148 \
  --label oo.session="$SESSION" \
  --label oo.task=copy-encoder-oracle-2 \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$RUST_IMAGE" \
  bash /inside.sh > "$ATTEMPT/console.log" 2>&1
oracle_exit=$?
set -e
if [ "$oracle_exit" -eq 125 ] && [ -f "$SNAP/Cargo.toml" ]; then
  cp "$ATTEMPT/console.log" "$ATTEMPT/console-125.log"
  sleep 2
  docker rm -f "$ORACLE_NAME" >/dev/null 2>&1 || true
  set +e
  docker run \
    --name "$ORACLE_NAME" \
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
    -e OO_A148_EVIDENCE=/evidence \
    --mount "type=bind,source=${SNAP},target=/src,readonly" \
    --mount "type=bind,source=${ORACLE_SRC}/inside.sh,target=/inside.sh,readonly" \
    --mount "type=bind,source=${ATTEMPT},target=/evidence" \
    --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
    --mount "type=volume,source=${TARGET_VOL},target=/target" \
    --label oo.project=open-ontology \
    --label oo.owner=a148 \
    --label oo.session="$SESSION" \
    --label oo.task=copy-encoder-oracle-3 \
    --label oo.toolchain=1.94.0 \
    --label oo.platform=linux/arm64 \
    --label oo.kind=rust-builder \
    "$RUST_IMAGE" \
    bash /inside.sh > "$ATTEMPT/console.log" 2>&1
  oracle_exit=$?
  set -e
fi
printf '%s\n' "$oracle_exit" > "$ATTEMPT/docker-run.exit"
docker rm "$ORACLE_NAME" >/dev/null || true
if [ "$oracle_exit" -ne 0 ]; then
  printf '%s\n' "FAILED oracle_exit=${oracle_exit}" > "$ATTEMPT/run-status.txt"
  exit "$oracle_exit"
fi

docker cp "$ATTEMPT/fresh.copy" "$PG_NAME":/tmp/fresh.copy
docker cp "$ATTEMPT/domain-violation.copy" "$PG_NAME":/tmp/domain-violation.copy
docker cp "$ORACLE_SRC/load.sql" "$PG_NAME":/tmp/load.sql
docker cp "$ORACLE_SRC/domain.sql" "$PG_NAME":/tmp/domain.sql

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

docker cp "$PG_NAME":/tmp/a148-version.txt "$ATTEMPT/pg-version.txt"
docker cp "$PG_NAME":/tmp/a148-count.txt "$ATTEMPT/pg-count.txt"
docker cp "$PG_NAME":/tmp/a148-schema.txt "$ATTEMPT/pg-schema.txt"
docker cp "$PG_NAME":/tmp/a148-rows.txt "$ATTEMPT/pg-rows.txt"
docker cp "$PG_NAME":/tmp/a148-rollback.txt "$ATTEMPT/pg-rollback.txt"

set +e
diff -u "$ATTEMPT/expected-schema.txt" "$ATTEMPT/pg-schema.txt" > "$ATTEMPT/schema.diff"
schema_diff=$?
diff -u "$ATTEMPT/expected-rows.txt" "$ATTEMPT/pg-rows.txt" > "$ATTEMPT/rows.diff"
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
