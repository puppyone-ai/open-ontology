#!/bin/bash
# Launch one a297 calendar-store review builder.
# Usage: launch.sh <evidence-dir> <mode> <container-name> <restart:yes|no>
# Origin shape: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh
# sha256 08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841
# Changes: uuid a297, fixed commit cc98bab7, 6g, 1 cpu, global cap 10, calendar archive, optional postgres restart.
set -u
if [ "$#" -ne 4 ]; then
  echo "usage: launch.sh <evidence-dir> <mode> <container-name> <restart:yes|no>" >&2
  exit 2
fi
SRC=/Users/redux/puppy-one/open-ontology
REVIEW=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a297/SP08/calendar-store-review
case "$1" in
  /*) EVIDENCE=$1 ;;
  *) EVIDENCE="$(pwd)/$1" ;;
esac
MODE=$2
NAME=$3
RESTART=$4
ROOT=/tmp/oo-a297-eef831aa-dba4-4af2-a78b-541bc9e0a297
FIX=${ROOT}/fixture
FIXED=cc98bab7eba2c58c127435db4d2ae777797259de
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
NET=oo-a297-eef831aa-net
CARGO_VOL=oo-a297-eef831aa-cargo
TARGET_VOL=oo-a297-eef831aa-target
PG_NAME=oo-a297-pg

mkdir -p "$EVIDENCE"
if [ -f "$EVIDENCE/console.log" ]; then
  echo "refusing to overwrite $EVIDENCE/console.log" >&2
  exit 2
fi
cp "$REVIEW/linux/inside.sh" "$EVIDENCE/inside.sh"
chmod 755 "$EVIDENCE/inside.sh" "$REVIEW/linux/launch.sh"
if [ ! -d "$FIX" ] || [ ! -f "$FIX/endpoint.txt" ]; then
  echo "fresh fixture is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ "$RESTART" = "yes" ]; then
  # The server has no postgres role. The fixture-admin password is read inside the
  # container from the mounted secret file, so it is not a host argv argument.
  postmaster_time() {
    docker exec -u postgres "$PG_NAME" bash -c 'export PGPASSWORD="$(tr -d "\n" < /secrets/fixture-admin.pass)"; psql -v ON_ERROR_STOP=1 -U oo_fixture_admin -d postgres -Atc "SELECT pg_postmaster_start_time()"' > "$1" 2> "$2"
  }
  postmaster_time "$EVIDENCE/postmaster-before.txt" "$EVIDENCE/postmaster-before.err"
  before_query=$?
  docker restart "$PG_NAME" > "$EVIDENCE/docker-restart.txt"
  ready=0
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
    if docker exec -u postgres "$PG_NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 2
  done
  postmaster_time "$EVIDENCE/postmaster-after.txt" "$EVIDENCE/postmaster-after.err"
  after_query=$?
  if [ "$ready" -ne 1 ] || [ "$before_query" -ne 0 ] || [ "$after_query" -ne 0 ]; then
    echo "postgres did not return a postmaster time after restart" >&2
    printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
    exit 1
  fi
  if ! grep -q '^[0-9]' "$EVIDENCE/postmaster-before.txt" || ! grep -q '^[0-9]' "$EVIDENCE/postmaster-after.txt"; then
    echo "postmaster time was empty" >&2
    printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
    exit 1
  fi
  if cmp -s "$EVIDENCE/postmaster-before.txt" "$EVIDENCE/postmaster-after.txt"; then
    echo "postmaster start time did not change" >&2
    printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
    exit 1
  fi
fi
rm -rf "$ROOT/archive" "$ROOT/work"
mkdir -p "$ROOT/archive" "$ROOT/work"
git -C "$SRC" archive --format=tar "$FIXED" \
  Cargo.toml Cargo.lock crates/oo-core tools/business-calendar tools/calendar-store examples/schema/calendar.yaml \
  | tar -x -C "$ROOT/archive"
if [ ! -f "$ROOT/archive/Cargo.toml" ]; then
  echo "archive missing root Cargo.toml" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse "${FIXED}:Cargo.toml" > "$EVIDENCE/root-cargo-toml.blob"
sha256sum "$ROOT/archive/Cargo.toml" > "$EVIDENCE/root-cargo-toml.host.sha256"
if [ ! -f "$ROOT/archive/tools/calendar-store/src/store.rs" ]; then
  echo "archive missing store.rs" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ ! -f "$ROOT/archive/tools/calendar-store/Cargo.lock" ]; then
  echo "archive missing the committed calendar-store lock" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
if [ -f "$ROOT/archive/tools/calendar-store/tests/a297_calendar.rs" ]; then
  echo "archive already contains the review overlay" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
git -C "$SRC" rev-parse HEAD > "$EVIDENCE/repo-head.txt"
printf '%s\n' "$FIXED" > "$EVIDENCE/fixed-commit.txt"
git -C "$SRC" rev-parse "${FIXED}^{tree}" > "$EVIDENCE/fixed-tree.txt"
git -C "$SRC" rev-parse "${FIXED}:tools/business-calendar" > "$EVIDENCE/helper-tree.txt"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/docker-run.exit"
  exit 1
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "private network is absent" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$EVIDENCE/network.txt"
docker volume inspect "$CARGO_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a297 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a297 \
  --label oo.kind=cargo-home \
  "$CARGO_VOL" >/dev/null
docker volume inspect "$TARGET_VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a297 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a297 \
  --label oo.kind=cargo-target \
  "$TARGET_VOL" >/dev/null
own=$(docker ps -q --filter label=oo.owner=a297 --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global_builders=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
own_pg=$(docker ps -q --filter label=oo.owner=a297 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s own_postgres=%s cap_global=10 cap_own=1 postgres=1\n' "$own" "$global_builders" "$host_cargo" "$own_pg" > "$EVIDENCE/capacity.txt"
docker ps --format '{{.ID}} {{.Names}} {{.Image}} {{.Label "oo.owner"}} {{.Label "oo.kind"}}' > "$EVIDENCE/containers-before.txt"
if [ "$own" -ge 1 ] || [ "$global_builders" -ge 10 ] || [ "$host_cargo" -ge 1 ] || [ "$own_pg" -ne 1 ]; then
  echo "deferred: rust builder or postgres capacity" >&2
  printf '%s\n' 3 > "$EVIDENCE/docker-run.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named proof container already exists" >&2
  printf '%s\n' 2 > "$EVIDENCE/docker-run.exit"
  exit 2
fi
printf '%s\n' "docker run --name ${NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 MODE=${MODE} bash /inside.sh" > "$EVIDENCE/invocation.txt"
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
  -e MODE="$MODE" \
  --mount "type=bind,source=${ROOT}/archive,target=/src,readonly" \
  --mount "type=bind,source=${ROOT}/work,target=/work" \
  --mount "type=bind,source=${REVIEW}/source/blobs,target=/blobs,readonly" \
  --mount "type=bind,source=${REVIEW}/overlay,target=/overlay,readonly" \
  --mount "type=bind,source=${REVIEW}/tools/lock_subset.rs,target=/tools/lock_subset.rs,readonly" \
  --mount "type=bind,source=${EVIDENCE}/inside.sh,target=/inside.sh,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${FIX},target=/run/oo-a289/fixture,readonly" \
  --mount "type=bind,source=${FIX},target=/run/oo-a297/fixture,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,noexec,size=512m \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project=open-ontology \
  --label oo.owner=a297 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a297 \
  --label oo.task=calendar-store-review \
  --label oo.toolchain=1.94.0 \
  --label oo.platform=linux/arm64 \
  --label oo.kind=rust-builder \
  "$IMAGE" \
  bash /inside.sh > "$EVIDENCE/console.log" 2>&1
run_exit=$?
set +e
printf '%s\n' "$run_exit" > "$EVIDENCE/docker-run.exit"
if docker container inspect "$NAME" >/dev/null 2>&1; then
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
else
  printf '%s\n' 'container was not created' > "$EVIDENCE/container-removed.txt"
fi
exit "$run_exit"
