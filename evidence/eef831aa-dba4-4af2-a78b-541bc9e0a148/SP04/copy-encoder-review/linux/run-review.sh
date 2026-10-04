#!/bin/bash
# a148 independent review runner. Smoke the immutable archive, then one private PG oracle.
# Removes only containers, volumes, and the network named for this session.
set -euo pipefail

SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a148/SP04/copy-encoder-review
SMOKE_EV=${EVIDENCE}/linux/smoke
ORACLE_EV=${EVIDENCE}/linux/oracle
IDENT=${EVIDENCE}/identity
ROOT=/tmp/oo-a148-eef831aa-dba4-4af2-a78b-541bc9e0a148
SMOKE_SNAP=${ROOT}/smoke-src
ORACLE_SNAP=${ROOT}/oracle-src
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
SMOKE_NAME=oo-a148-smoke
ORACLE_NAME=oo-a148-oracle
PG_NAME=oo-a148-pg
NET=oo-a148-eef831aa-net
CARGO_VOL=oo-a148-eef831aa-cargo
TARGET_VOL=oo-a148-eef831aa-target
PG_VOL=oo-a148-eef831aa-pg
FIXED=d80472d8a99afd66d8d0e71420f7be2bb62e65c8
EXPECT_TREE=a3b10827acfac28c4abf986831cbcb26823477dc
EXPECT_PARENT=8e0d1da658d65878be96f6f2a7a4478dd3222b19
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a148
success=0
created_net=0
created_pg_vol=0

mkdir -p "$SMOKE_EV" "$ORACLE_EV" "$IDENT" "$ROOT"
if [ -f "$SMOKE_EV/console.log" ] || [ -f "$ORACLE_EV/console.log" ]; then
  echo "refusing to overwrite an existing console.log" >&2
  exit 2
fi
chmod 755 "$SMOKE_EV/inside.sh" "$ORACLE_EV/inside.sh" "$EVIDENCE/linux/run-review.sh"

cleanup() {
  {
    echo "cleanup_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "success=${success}"
    docker inspect "$SMOKE_NAME" --format 'name=smoke ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' \
      > "$SMOKE_EV/inspect.txt" 2>/dev/null || true
    docker inspect "$SMOKE_NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} memory={{.HostConfig.Memory}}' \
      > "$SMOKE_EV/hostconfig.txt" 2>/dev/null || true
    docker inspect "$ORACLE_NAME" --format 'name=oracle ExitCode={{.State.ExitCode}} Status={{.State.Status}} OOMKilled={{.State.OOMKilled}} Error={{.State.Error}}' \
      > "$ORACLE_EV/inspect.txt" 2>/dev/null || true
    docker inspect "$ORACLE_NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} security={{json .HostConfig.SecurityOpt}} memory={{.HostConfig.Memory}}' \
      > "$ORACLE_EV/hostconfig.txt" 2>/dev/null || true
    docker inspect "$PG_NAME" --format 'privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} net={{.HostConfig.NetworkMode}} tmpfs={{json .HostConfig.Tmpfs}} memory={{.HostConfig.Memory}}' \
      > "$ORACLE_EV/postgres-hostconfig.txt" 2>/dev/null || true
    docker logs "$PG_NAME" > "$ORACLE_EV/postgres.log" 2>&1 || true
    echo "rm_containers:"
    docker rm -f "$SMOKE_NAME" "$ORACLE_NAME" "$PG_NAME" 2>&1 || true
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
    echo "rm_snapshot:"
    rm -rf "$ROOT"
    echo "snapshot_removed=1"
    echo "remaining_a148:"
    docker ps -a --filter label=oo.owner=a148 --format 'name={{.Names}} status={{.Status}}' 2>&1 || true
    docker volume ls --filter label=oo.owner=a148 --format '{{.Name}}' 2>&1 || true
    docker network ls --filter label=oo.owner=a148 --format '{{.Name}}' 2>&1 || true
  } > "$EVIDENCE/linux/cleanup.txt" 2>&1 || true
  if [ "$success" -eq 1 ]; then
    printf '%s\n' "DONE" > "$EVIDENCE/linux/run-status.txt"
  elif [ ! -f "$EVIDENCE/linux/run-status.txt" ]; then
    printf '%s\n' "FAILED" > "$EVIDENCE/linux/run-status.txt"
  fi
}
trap cleanup EXIT

file_sha() {
  shasum -a 256 "$1" | awk '{print $1}'
}

capacity_gate() {
  local where=$1
  local dest=$2
  {
    echo "observed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "where=${where}"
    docker ps --filter label=oo.project=open-ontology --format 'name={{.Names}} kind={{.Label "oo.kind"}} owner={{.Label "oo.owner"}} status={{.Status}}'
    echo "host_toolchain_comm:"
    ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /^(cargo|rustc|rustdoc|clippy-driver|rustup)$/ {print}' || true
  } > "$dest"
  local running_rust own host_cargo
  running_rust=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | sed '/^$/d' | wc -l | tr -d ' ')
  own=$(docker ps -q --filter label=oo.owner=a148 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
  host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" || $1=="clippy-driver" {c++} END{print c+0}')
  printf 'running_rust_builders=%s own_rust_builder_running=%s host_cargo_rustc_rustdoc_rustup=%s\n' "$running_rust" "$own" "$host_cargo" > "${dest%.txt}-capacity.txt"
  if [ "$own" -ge 1 ] || [ "$((running_rust + host_cargo))" -ge 2 ]; then
    echo "deferred: rust builder capacity is full" >&2
    printf '%s\n' 3 > "$EVIDENCE/linux/capacity.exit"
    exit 3
  fi
  printf '%s\n' 0 > "$EVIDENCE/linux/capacity.exit"
}

head_now=$(git -C "$SRC" rev-parse HEAD)
printf '%s\n' "$head_now" > "$IDENT/workspace-head.txt"
actual_tree=$(git -C "$SRC" rev-parse "${FIXED}^{tree}")
actual_parent=$(git -C "$SRC" rev-parse "${FIXED}^")
printf '%s\n' "$FIXED" > "$IDENT/commit.txt"
printf '%s\n' "$actual_tree" > "$IDENT/tree.txt"
printf '%s\n' "$actual_parent" > "$IDENT/parent.txt"
test "$actual_tree" = "$EXPECT_TREE"
test "$actual_parent" = "$EXPECT_PARENT"
git -C "$SRC" diff --exit-code "$FIXED" -- \
  contracts/pipeline/copy-encoder-v1.md \
  crates/oo-pipeline/src/copy_encoder.rs \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/tests/copy_encoder_live.rs \
  > "$IDENT/encoder-diff.txt"

{
  echo "commit=${FIXED}"
  echo "tree=${actual_tree}"
  echo "parent=${actual_parent}"
  echo "workspace_head=${head_now}"
  for rel in \
    contracts/pipeline/copy-encoder-v1.md \
    crates/oo-pipeline/src/copy_encoder.rs \
    crates/oo-pipeline/src/lib.rs \
    crates/oo-pipeline/tests/copy_encoder_live.rs \
    crates/oo-pipeline/src/physical_mapping.rs \
    crates/oo-pipeline/Cargo.toml \
    contracts/source/pgsql/type-conversion-v1.md
  do
    printf 'blob %s %s\n' "$(git -C "$SRC" rev-parse "${FIXED}:${rel}")" "$rel"
  done
} > "$IDENT/blobs.txt"
test "$(git -C "$SRC" rev-parse "${FIXED}:contracts/pipeline/copy-encoder-v1.md")" = "9b1d8f12be86675f35604bbb2e31d5fa3db57248"
test "$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/src/copy_encoder.rs")" = "8eb2224204ea3f6e57d8407ce218bab790a8059d"
test "$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/src/lib.rs")" = "2e0fa8e0fe4de6f2d72ae2e7b73cd4881805c556"
test "$(git -C "$SRC" rev-parse "${FIXED}:crates/oo-pipeline/tests/copy_encoder_live.rs")" = "2021732e21dac0ff7a280336c3a5e44fb44238af"

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$RUST_IMAGE" > "$IDENT/rust-image.txt"
grep -q 'arch=arm64' "$IDENT/rust-image.txt"
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$PG_IMAGE" > "$IDENT/postgres-image.txt"
grep -q 'arch=arm64' "$IDENT/postgres-image.txt"

capacity_gate before-smoke "$EVIDENCE/linux/builders-before.txt"

if docker container inspect "$SMOKE_NAME" >/dev/null 2>&1 \
  || docker container inspect "$ORACLE_NAME" >/dev/null 2>&1 \
  || docker container inspect "$PG_NAME" >/dev/null 2>&1; then
  echo "named a148 container already exists" >&2
  exit 2
fi

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a148 \
    --label oo.session="$SESSION" \
    --label oo.kind=network \
    "$NET" >/dev/null
  created_net=1
fi

ensure_volume() {
  local name=$1
  local kind=$2
  if docker volume inspect "$name" >/dev/null 2>&1; then
    local owner
    owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$name")
    if [ "$owner" != "a148" ]; then
      echo "refusing foreign volume ${name}" >&2
      exit 2
    fi
  else
    docker volume create \
      --label oo.project=open-ontology \
      --label oo.owner=a148 \
      --label oo.session="$SESSION" \
      --label oo.kind="$kind" \
      "$name" >/dev/null
  fi
}
ensure_volume "$CARGO_VOL" cargo
ensure_volume "$TARGET_VOL" target

rm -rf "$SMOKE_SNAP" "$ORACLE_SNAP"
mkdir -p "$SMOKE_SNAP" "$ORACLE_SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$SMOKE_SNAP"
git -C "$SRC" archive "$FIXED" | tar -x -C "$ORACLE_SNAP"
cp "$ORACLE_EV/a148_copy_oracle.rs" "$ORACLE_SNAP/crates/oo-pipeline/tests/a148_copy_oracle.rs"
test ! -f "$SMOKE_SNAP/crates/oo-pipeline/tests/a148_copy_oracle.rs"
head_after=$(git -C "$SRC" rev-parse HEAD)
test "$head_after" = "$head_now"

for rel in \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/src/copy_encoder.rs \
  crates/oo-pipeline/tests/copy_encoder_live.rs \
  crates/oo-pipeline/src/physical_mapping.rs \
  crates/oo-pipeline/Cargo.toml \
  contracts/pipeline/copy-encoder-v1.md \
  contracts/source/pgsql/type-conversion-v1.md
do
  test "$(git -C "$SRC" hash-object "$SMOKE_SNAP/$rel")" = "$(git -C "$SRC" rev-parse "${FIXED}:${rel}")"
  test "$(git -C "$SRC" hash-object "$ORACLE_SNAP/$rel")" = "$(git -C "$SRC" rev-parse "${FIXED}:${rel}")"
done

file_sha "$SMOKE_SNAP/crates/oo-pipeline/src/lib.rs" > "$SMOKE_EV/expect-lib.sha256"
file_sha "$SMOKE_SNAP/crates/oo-pipeline/src/copy_encoder.rs" > "$SMOKE_EV/expect-encoder.sha256"
file_sha "$SMOKE_SNAP/crates/oo-pipeline/tests/copy_encoder_live.rs" > "$SMOKE_EV/expect-live.sha256"
file_sha "$SMOKE_SNAP/crates/oo-pipeline/src/physical_mapping.rs" > "$SMOKE_EV/expect-mapping.sha256"
file_sha "$SMOKE_SNAP/crates/oo-pipeline/Cargo.toml" > "$SMOKE_EV/expect-cargo.sha256"
file_sha "$SMOKE_SNAP/contracts/pipeline/copy-encoder-v1.md" > "$SMOKE_EV/expect-contract.sha256"
file_sha "$SMOKE_SNAP/contracts/source/pgsql/type-conversion-v1.md" > "$SMOKE_EV/expect-types.sha256"
cp "$SMOKE_EV"/expect-*.sha256 "$ORACLE_EV"/
file_sha "$ORACLE_SNAP/crates/oo-pipeline/tests/a148_copy_oracle.rs" > "$ORACLE_EV/expect-overlay.sha256"
{
  echo "smoke_overlay=0"
  echo "oracle_overlay=tests/a148_copy_oracle.rs"
  echo "production_blobs_match_fixed=1"
} > "$IDENT/snapshot-notes.txt"

cat > "$EVIDENCE/linux/invocation.txt" <<EOF
source_commit=${FIXED}
tree=${actual_tree}
parent=${actual_parent}
workspace_head=${head_now}
smoke_composition=git archive of ${FIXED} with no overlay
oracle_composition=git archive of ${FIXED} plus tests/a148_copy_oracle.rs
rust_image=${RUST_IMAGE}
postgres_image=${PG_IMAGE}
platform=linux/arm64
toolchain=1.94.0
smoke_container=${SMOKE_NAME}
oracle_container=${ORACLE_NAME}
postgres=${PG_NAME}
network=${NET}
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
rust_memory=8g
postgres_memory=1g
phases=smoke_fetch smoke_test smoke_clippy oracle_test oracle_clippy psql_decode domain_check
package=oo-pipeline
EOF

run_rust() {
  local name=$1
  local snap=$2
  local evidence=$3
  local task=$4
  local inside=$5
  capacity_gate "before-${task}" "$EVIDENCE/linux/builders-before-${task}.txt"
  docker run \
    --name "$name" \
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
    --mount "type=bind,source=${snap},target=/src,readonly" \
    --mount "type=bind,source=${inside},target=/inside.sh,readonly" \
    --mount "type=bind,source=${evidence},target=/evidence" \
    --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
    --mount "type=volume,source=${TARGET_VOL},target=/target" \
    --label oo.project=open-ontology \
    --label oo.owner=a148 \
    --label oo.session="$SESSION" \
    --label oo.task="$task" \
    --label oo.toolchain=1.94.0 \
    --label oo.platform=linux/arm64 \
    --label oo.kind=rust-builder \
    "$RUST_IMAGE" \
    bash /inside.sh > "$evidence/console.log" 2>&1
}

set +e
run_rust "$SMOKE_NAME" "$SMOKE_SNAP" "$SMOKE_EV" copy-encoder-smoke "$SMOKE_EV/inside.sh"
smoke_exit=$?
set -e
printf '%s\n' "$smoke_exit" > "$SMOKE_EV/docker-run.exit"
if [ "$smoke_exit" -ne 0 ]; then
  printf '%s\n' "FAILED smoke_exit=${smoke_exit}" > "$EVIDENCE/linux/run-status.txt"
  exit "$smoke_exit"
fi
docker rm "$SMOKE_NAME" >/dev/null

if docker volume inspect "$PG_VOL" >/dev/null 2>&1; then
  owner=$(docker volume inspect -f '{{index .Labels "oo.owner"}}' "$PG_VOL")
  if [ "$owner" != "a148" ]; then
    echo "refusing foreign postgres volume" >&2
    exit 2
  fi
  echo "refusing a pre-existing postgres volume" >&2
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a148 \
  --label oo.session="$SESSION" \
  --label oo.kind=postgres \
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
  --label oo.task=copy-encoder-oracle \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  "$PG_IMAGE" \
  -c listen_addresses='*' -c max_connections=40 -c shared_preload_libraries= \
  > "$ORACLE_EV/postgres-start.txt"

ready=0
for _ in $(seq 1 60); do
  if docker exec "$PG_NAME" pg_isready -U postgres -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
printf 'ready=%s\n' "$ready" > "$ORACLE_EV/postgres-ready.txt"
if [ "$ready" -ne 1 ]; then
  printf '%s\n' "FAILED postgres_not_ready" > "$EVIDENCE/linux/run-status.txt"
  exit 1
fi

set +e
run_rust "$ORACLE_NAME" "$ORACLE_SNAP" "$ORACLE_EV" copy-encoder-oracle "$ORACLE_EV/inside.sh"
oracle_exit=$?
set -e
printf '%s\n' "$oracle_exit" > "$ORACLE_EV/docker-run.exit"
docker rm "$ORACLE_NAME" >/dev/null
if [ "$oracle_exit" -ne 0 ]; then
  printf '%s\n' "FAILED oracle_exit=${oracle_exit}" > "$EVIDENCE/linux/run-status.txt"
  exit "$oracle_exit"
fi

docker cp "$ORACLE_EV/fresh.copy" "$PG_NAME":/tmp/fresh.copy
docker cp "$ORACLE_EV/domain-violation.copy" "$PG_NAME":/tmp/domain-violation.copy
docker cp "$ORACLE_EV/load.sql" "$PG_NAME":/tmp/load.sql
docker cp "$ORACLE_EV/domain.sql" "$PG_NAME":/tmp/domain.sql

set +e
docker exec -u postgres "$PG_NAME" psql -U postgres -d postgres -f /tmp/load.sql \
  > "$ORACLE_EV/psql-main.stdout" 2> "$ORACLE_EV/psql-main.stderr"
psql_main=$?
set -e
printf '%s\n' "$psql_main" > "$ORACLE_EV/psql-main.exit"
if [ "$psql_main" -ne 0 ]; then
  printf '%s\n' "FAILED psql_main=${psql_main}" > "$EVIDENCE/linux/run-status.txt"
  exit "$psql_main"
fi

docker cp "$PG_NAME":/tmp/a148-version.txt "$ORACLE_EV/pg-version.txt"
docker cp "$PG_NAME":/tmp/a148-count.txt "$ORACLE_EV/pg-count.txt"
docker cp "$PG_NAME":/tmp/a148-schema.txt "$ORACLE_EV/pg-schema.txt"
docker cp "$PG_NAME":/tmp/a148-rows.txt "$ORACLE_EV/pg-rows.txt"
docker cp "$PG_NAME":/tmp/a148-rollback.txt "$ORACLE_EV/pg-rollback.txt"

set +e
diff -u "$ORACLE_EV/expected-schema.txt" "$ORACLE_EV/pg-schema.txt" > "$ORACLE_EV/schema.diff"
schema_diff=$?
diff -u "$ORACLE_EV/expected-rows.txt" "$ORACLE_EV/pg-rows.txt" > "$ORACLE_EV/rows.diff"
rows_diff=$?
set -e
printf '%s\n' "$schema_diff" > "$ORACLE_EV/schema-diff.exit"
printf '%s\n' "$rows_diff" > "$ORACLE_EV/rows-diff.exit"
grep -q '17.10' "$ORACLE_EV/pg-version.txt"
grep -q '170010' "$ORACLE_EV/pg-version.txt"
grep -q '^2$' "$ORACLE_EV/pg-count.txt"
grep -q '^null$' "$ORACLE_EV/pg-rollback.txt"
if [ "$schema_diff" -ne 0 ] || [ "$rows_diff" -ne 0 ]; then
  printf '%s\n' "FAILED compare schema=${schema_diff} rows=${rows_diff}" > "$EVIDENCE/linux/run-status.txt"
  exit 1
fi

set +e
docker exec -u postgres "$PG_NAME" psql -U postgres -d postgres -f /tmp/domain.sql \
  > "$ORACLE_EV/psql-domain.stdout" 2> "$ORACLE_EV/psql-domain.stderr"
psql_domain=$?
set -e
printf '%s\n' "$psql_domain" > "$ORACLE_EV/psql-domain.exit"
if [ "$psql_domain" -eq 0 ]; then
  echo "domain check unexpectedly accepted the row" >&2
  printf '%s\n' "FAILED domain_accepted" > "$EVIDENCE/linux/run-status.txt"
  exit 1
fi
if ! grep -q '23514' "$ORACLE_EV/psql-domain.stderr"; then
  echo "domain failure was not SQLSTATE 23514" >&2
  printf '%s\n' "FAILED domain_sqlstate" > "$EVIDENCE/linux/run-status.txt"
  exit 1
fi

docker exec -u postgres "$PG_NAME" psql -U postgres -d postgres -v ON_ERROR_STOP=1 -At -F '|' -c \
  "SELECT CASE WHEN to_regclass('oo_copy_a148.fresh') IS NULL THEN 'null' ELSE 'present' END, CASE WHEN to_regclass('oo_copy_a148_chk.checked') IS NULL THEN 'null' ELSE 'present' END, CASE WHEN to_regclass('oo_copy_a148_chk.pos_int') IS NULL THEN 'null' ELSE 'present' END;" \
  > "$ORACLE_EV/pg-after.txt"
grep -Fxq 'null|null|null' "$ORACLE_EV/pg-after.txt"

success=1
printf '%s\n' "DONE" > "$EVIDENCE/linux/run-status.txt"
exit 0
