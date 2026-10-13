#!/bin/bash
# Isolated ClickHouse C4 capture into the fixed native loader.
# No CPU quota, no memory quota, no admit.py. Old volumes stay unmounted.
# Cleanup removes only this run's two containers. Volumes are kept.
set -u
ROOT="/Users/redux/puppy-one/open-ontology"
EV="$ROOT/evidence/4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de/DL01/clickhouse-product/raw/run/product-run-16-native"
SCRIPTS="$ROOT/tools/clickhouse-preview/scripts"
SECRETS="/tmp/oo-dl01w1-40-return/secrets"
LIMITS="$ROOT/testing/dl01-driver-preflight/clickhouse/config/limits.xml"
PWDIR="/tmp/oo-dl01w1-40-native"
PWFILE="$PWDIR/pg.password"
ELF="/tmp/oo-u06-194/debug/oo-pipeline"
ELF_SHA="4673c8b6e58526c46cac9697b13fd015601992179ac6c38f0e21cf3566acabfe"
CH_IMAGE="clickhouse/clickhouse-server@sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa"
PG_IMAGE="postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb"
RUST_IMAGE="rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5"
NET="oo-dl01w1-40-native2-net"
CH_NAME="oo-dl01w1-40-native2-ch"
PG_NAME="oo-dl01w1-40-native2-pg"
CH_VOL="oo-dl01w1-40-native2-chdata"
PG_VOL="oo-dl01w1-40-native2-pgdata"
mkdir -p "$EV" "$EV/logs"
exec >>"$EV/launcher.log" 2>&1
echo "NATIVE_START $(date -u +%Y-%m-%dT%H:%M:%SZ)"
fail() {
  printf '%s\n' "$1" > "$EV/NATIVE_FAIL"
  echo "NATIVE_FAIL $1"
  exit 1
}
cleanup() {
  docker logs "$CH_NAME" > "$EV/logs/clickhouse.log" 2>&1 || true
  docker logs "$PG_NAME" > "$EV/logs/postgres.log" 2>&1 || true
  docker stop "$CH_NAME" "$PG_NAME" >/dev/null 2>&1 || true
  docker rm "$CH_NAME" "$PG_NAME" >/dev/null 2>&1 || true
  docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/foreign-pg-after.txt" 2>/dev/null || true
  docker volume ls --filter name=oo-dl01w1-40 --format '{{.Name}}' > "$EV/volumes-kept.txt" 2>/dev/null || true
  echo "NATIVE_CLEANUP $(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
trap cleanup EXIT
test -f "$SECRETS/reader.password" || fail "reader secret file missing"
test -f "$SECRETS/dba.password" || fail "dba secret file missing"
test -f "$SECRETS/users.d/oo-users.xml" || fail "users xml missing"
test -f "$LIMITS" || fail "limits xml missing"
test -x "$ELF" || fail "native elf missing or not executable"
got_elf="$(shasum -a 256 "$ELF" | awk '{print $1}')"
printf '%s\n' "$got_elf" > "$EV/elf.sha256"
test "$got_elf" = "$ELF_SHA" || fail "elf sha mismatch"
docker image inspect "$CH_IMAGE" --format '{{.Id}} {{.Os}}/{{.Architecture}}' > "$EV/image-clickhouse.txt" || fail "clickhouse image missing"
docker image inspect "$PG_IMAGE" --format '{{.Id}} {{.Os}}/{{.Architecture}}' > "$EV/image-postgres.txt" || fail "postgres image missing"
docker image inspect "$RUST_IMAGE" --format '{{.Id}} {{.Os}}/{{.Architecture}}' > "$EV/image-rust.txt" || fail "rust image missing"
docker inspect oo-dl01w1-06-pg --format '{{.Id}} {{.State.Status}}' > "$EV/foreign-pg-before.txt" || fail "foreign pg missing"
grep -q ' running$' "$EV/foreign-pg-before.txt" || fail "foreign pg is not running"
if docker ps -a --format '{{.Names}}' | grep -Eq "^(${CH_NAME}|${PG_NAME})$"; then
  fail "native container name already present"
fi
if docker volume ls --format '{{.Name}}' | grep -Eq "^(${CH_VOL}|${PG_VOL})$"; then
  fail "native volume already present"
fi
if docker network ls --format '{{.Name}}' | grep -Eq "^${NET}$"; then
  fail "native network already present"
fi
sysctl -n hw.ncpu > "$EV/host-ncpu.txt"
sysctl -n hw.memsize > "$EV/host-memsize.txt"
printf '%s\n' "admit.py was not called. The 2026-10-12 human authorization lifts the CPU and compile-slot gates for this run." > "$EV/cpu-authorization.txt"
cd "$SCRIPTS"
python3 -m py_compile capture_receipt.py run_native_chain.py || fail "py_compile"
python3 -m unittest test_capture_receipt.py test_c4_capture.py > "$EV/unit.stdout" 2> "$EV/unit.stderr"
printf '%s\n' "$?" > "$EV/unit.exit"
test "$(tr -d '[:space:]' < "$EV/unit.exit")" = "0" || fail "unit tests"
mkdir -p "$PWDIR"
umask 077
openssl rand -hex 24 > "$PWFILE"
chmod 600 "$PWFILE"
docker network create --subnet 172.16.141.0/24 --gateway 172.16.141.1 "$NET" > "$EV/network-create.txt" || fail "network create"
docker volume create "$CH_VOL" > "$EV/volume-ch.txt" || fail "clickhouse volume"
docker volume create "$PG_VOL" > "$EV/volume-pg.txt" || fail "postgres volume"
docker run -d --name "$CH_NAME" \
  --platform linux/arm64 --pull never \
  --network "$NET" \
  --ulimit nofile=262144:262144 \
  --mount "type=volume,source=${CH_VOL},target=/var/lib/clickhouse" \
  --mount "type=bind,source=${LIMITS},target=/etc/clickhouse-server/config.d/oo-limits.xml,readonly" \
  --mount "type=bind,source=${SECRETS}/users.d/oo-users.xml,target=/etc/clickhouse-server/users.d/oo-users.xml,readonly" \
  --mount "type=bind,source=${SECRETS}/dba.password,target=/etc/oo/dba.password,readonly" \
  --mount "type=bind,source=${SECRETS}/reader.password,target=/etc/oo/reader.password,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=oo-dl01w1-40 \
  --label oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
  --label oo.task=DL01-W1-40 \
  --label oo.kind=clickhouse-source \
  "$CH_IMAGE" > "$EV/clickhouse-container.txt" || fail "clickhouse start"
docker run -d --name "$PG_NAME" \
  --platform linux/arm64 --pull never \
  --network "$NET" \
  --ulimit nofile=262144:262144 \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=oo_ch40 \
  --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
  --label oo.project=open-ontology \
  --label oo.owner=oo-dl01w1-40 \
  --label oo.session=4dbc6d5c-aeeb-46fe-9a28-6cc12626e2de \
  --label oo.task=DL01-W1-40 \
  --label oo.kind=native-agent-db \
  "$PG_IMAGE" > "$EV/postgres-container.txt" || fail "postgres start"
echo "DATABASES_STARTED $(date -u +%Y-%m-%dT%H:%M:%SZ)"
export OO_NATIVE_OUT="$EV"
export OO_NATIVE_CH="$CH_NAME"
export OO_NATIVE_PG="$PG_NAME"
export OO_NATIVE_ELF="$ELF"
export OO_NATIVE_PG_PASSWORD="$PWFILE"
export OO_NATIVE_RUST_IMAGE="$RUST_IMAGE"
export OO_NATIVE_SECRET="$SECRETS/reader.password"
python3 "$SCRIPTS/run_native_chain.py"
code=$?
printf '%s\n' "$code" > "$EV/launcher.exit"
echo "NATIVE_PY_EXIT $code $(date -u +%Y-%m-%dT%H:%M:%SZ)"
exit "$code"
