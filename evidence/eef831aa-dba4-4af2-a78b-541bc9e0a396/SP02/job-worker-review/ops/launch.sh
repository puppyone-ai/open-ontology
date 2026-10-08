#!/bin/bash
# Independent review runner. Recipe shape from tools/job-worker/ops/launch.sh
# at product 1c4797e2f846da7744565c0f95103b68c1d01846.
# Does not pull, prune, publish a host port, mount the docker socket, or mount host home.
# Does not run the author's ignored 100-worker closed_loop.
set -eu
set -o pipefail

MODE=${1:-}
HERE=$(cd "$(dirname "$0")" && pwd)
EVIDENCE=$(cd "$HERE/.." && pwd)
REPO=$(cd "$EVIDENCE/../../../.." && pwd)
OWNER=a396
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a396
PROJECT=open-ontology
NET=oo-a396-jw
PG_NAME=oo-a396-jw-pg
RUST_NAME=oo-a396-jw-rust
PG_VOL=oo-a396-jw-pgdata
CARGO_VOL=oo-a396-jw-cargo
TARGET_VOL=oo-a396-jw-target
PG_HOST=pg
PHASE="$EVIDENCE/linux/run"
STAGE=/tmp/oo-a396-$RUN_ID/stage-$(date +%s)
SEC=$STAGE/secrets
SRC=$STAGE/src
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
FIXED_SOURCE=1c4797e2f846da7744565c0f95103b68c1d01846

case "$MODE" in
  test|down) ;;
  *)
    echo "usage: launch.sh test|down" >&2
    exit 2
    ;;
esac

label_of() {
  local kind=$1 name=$2 key=$3
  if [ "$kind" = "container" ]; then
    docker container inspect "$name" --format "{{index .Config.Labels \"$key\"}}" 2>/dev/null || true
  else
    docker "$kind" inspect "$name" --format "{{index .Labels \"$key\"}}" 2>/dev/null || true
  fi
}

object_state() {
  local kind=$1 name=$2 owner session project
  if ! docker "$kind" inspect "$name" >/dev/null 2>&1; then
    echo missing
    return
  fi
  owner=$(label_of "$kind" "$name" oo.owner)
  session=$(label_of "$kind" "$name" oo.session)
  project=$(label_of "$kind" "$name" oo.project)
  if [ "$owner" = "$OWNER" ] && [ "$session" = "$RUN_ID" ] && [ "$project" = "$PROJECT" ]; then
    echo ours
  else
    echo foreign
  fi
}

remove_ours() {
  local kind=$1 name=$2 state
  state=$(object_state "$kind" "$name")
  case "$state" in
    missing) echo "absent $kind $name" ;;
    ours)
      if [ "$kind" = "container" ]; then
        docker rm -f "$name" >/dev/null
      elif [ "$kind" = "volume" ]; then
        docker volume rm "$name" >/dev/null
      else
        docker network rm "$name" >/dev/null
      fi
      echo "removed $kind $name"
      ;;
    foreign)
      echo "refusing foreign $kind $name" >&2
      return 1
      ;;
  esac
}

cleanup_owned() {
  local name owner session
  for name in "$RUST_NAME" "$PG_NAME" oo-a396-jw-smoke oo-a396-jw-smoke-cargo; do
    if docker container inspect "$name" >/dev/null 2>&1; then
      owner=$(label_of container "$name" oo.owner)
      session=$(label_of container "$name" oo.session)
      if [ "$owner" = "$OWNER" ] && [ "$session" = "$RUN_ID" ]; then
        docker rm -f "$name" >/dev/null
        echo "removed container $name"
      else
        echo "left container $name owner=${owner:-empty} session=${session:-empty}"
      fi
    fi
  done
  remove_ours volume "$PG_VOL" || true
  remove_ours volume "$CARGO_VOL" || true
  remove_ours volume "$TARGET_VOL" || true
  remove_ours network "$NET" || true
  if [ -d "$SEC" ]; then
    rm -rf "$SEC"
    echo "removed secrets dir"
  fi
  if [ -d "$SRC" ]; then
    rm -rf "$SRC"
    echo "removed source archive dir"
  fi
  if [ -n "${STAGE:-}" ] && [ -d "$STAGE" ]; then
    rm -rf "$STAGE"
    echo "removed stage dir"
  fi
}

if [ "$MODE" = "down" ]; then
  mkdir -p "$EVIDENCE/linux"
  set +e
  cleanup_owned > "$EVIDENCE/linux/cleanup.txt" 2>&1
  code=$?
  set -e
  printf '%s\n' "$code" > "$EVIDENCE/linux/cleanup.exit"
  exit "$code"
fi

mkdir -p "$PHASE" "$EVIDENCE/linux/smoke-before"
if [ -d "/tmp/oo-a396-$RUN_ID" ]; then
  cp -p /tmp/oo-a396-$RUN_ID/rustc.txt /tmp/oo-a396-$RUN_ID/rustc.err /tmp/oo-a396-$RUN_ID/rustc.exit \
    /tmp/oo-a396-$RUN_ID/cargo.txt /tmp/oo-a396-$RUN_ID/cargo.err /tmp/oo-a396-$RUN_ID/cargo.exit \
    /tmp/oo-a396-$RUN_ID/host-uname.txt /tmp/oo-a396-$RUN_ID/smoke-inspect.txt \
    /tmp/oo-a396-$RUN_ID/smoke-rm.txt "$EVIDENCE/linux/smoke-before/" 2>/dev/null || true
fi

own=$(docker ps -q --filter label=oo.owner="$OWNER" --filter label=oo.session="$RUN_ID" --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
global=$(docker ps -q --filter label=oo.kind=rust-builder | sed '/^$/d' | wc -l | tr -d ' ')
host_cargo=$(ps -axo comm= | awk '$1=="cargo" || $1=="rustc" || $1=="rustdoc" || $1=="rustup" {c++} END{print c+0}')
printf 'own=%s global=%s host_cargo=%s cap_global=10\n' "$own" "$global" "$host_cargo" > "$PHASE/capacity.txt"
if [ "$own" -ge 1 ] || [ "$global" -ge 10 ] || [ "$host_cargo" -ge 1 ]; then
  echo "deferred: builder capacity own=$own global=$global host_cargo=$host_cargo" >&2
  printf '%s\n' 3 > "$PHASE/docker-run.exit"
  exit 3
fi
for image in "$RUST_IMAGE" "$PG_IMAGE"; do
  if ! docker image inspect "$image" >/dev/null 2>&1; then
    echo "pinned image is not present locally; not pulling" >&2
    printf '%s\n' 1 > "$PHASE/docker-run.exit"
    exit 1
  fi
done
docker image inspect "$RUST_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PHASE/rust-image.txt"
docker image inspect "$PG_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PHASE/pg-image.txt"
grep -q 'arch=arm64' "$PHASE/rust-image.txt"
grep -q 'arch=arm64' "$PHASE/pg-image.txt"

rm -rf "$STAGE"
mkdir -p "$SRC" "$STAGE"
git -C "$REPO" archive "$FIXED_SOURCE" tools/job-worker crates/oo-jobs/migrations > "$STAGE/src.tar"
git -C "$REPO" get-tar-commit-id < "$STAGE/src.tar" > "$PHASE/archive-commit.txt"
tar -x -C "$SRC" -f "$STAGE/src.tar"
rm -f "$STAGE/src.tar"
test -f "$SRC/tools/job-worker/ops/pg-wrap.sh"
ls -ld "$SRC/tools/job-worker/ops" "$SRC/tools/job-worker/ops/pg-wrap.sh" > "$PHASE/src-preflight.txt"

{
  echo "fixed_source=$FIXED_SOURCE"
  echo "launch_origin=tools/job-worker/ops/launch.sh"
  echo "launch_commit=$FIXED_SOURCE"
  echo "launch_path=tools/job-worker/ops/launch.sh"
  echo "pg_wrap_origin=tools/job-worker/ops/pg-wrap.sh"
  echo "pg_wrap_sha256=$(sha256sum "$SRC/tools/job-worker/ops/pg-wrap.sh" | awk '{print $1}')"
  echo "pg_wrap_deviation=max_connections=128 copied unchanged from the product file; fixture capacity only"
  echo "reviewed_pg_wrap=deploy/trusted-demo/pg/pg-wrap.sh"
  echo "reviewed_pg_wrap_commit=10b78993641c88154c75c22219c79c1566e2cecd"
  echo "reviewed_pg_wrap_sha256=05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a"
  echo "secrets_path=$SEC"
} > "$EVIDENCE/recipe-origin.txt"
cp "$EVIDENCE/recipe-origin.txt" "$PHASE/recipe-origin.txt"

finished=0
finish() {
  if [ "$finished" -eq 1 ]; then
    return
  fi
  finished=1
  if docker container inspect "$PG_NAME" >/dev/null 2>&1; then
    docker logs "$PG_NAME" > "$PHASE/postgres.log" 2>&1 || true
    docker inspect "$PG_NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}}' > "$PHASE/pg-inspect.txt" || true
  fi
  cleanup_owned > "$EVIDENCE/linux/cleanup.txt" 2>&1
  printf '%s\n' "$?" > "$EVIDENCE/linux/cleanup.exit"
}
trap finish EXIT

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/runtime.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-${OWNER}-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=${PG_HOST}" >/dev/null 2>&1
printf 'basicConstraints=CA:FALSE\nsubjectAltName=DNS:%s\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' "$PG_HOST" > "$SEC/san.cnf"
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
chmod 600 "$SEC/server.key" "$SEC/ca.key" "$SEC"/*.pass
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$PHASE/server-san.txt"
cat > "$SEC/jw.conf" <<EOF
host=$PG_HOST
port=5432
database=oo_a390_control
runtime_user=oo_a390_runtime_login
runtime_password_file=/secrets/runtime.pass
admin_user=oo_fixture_admin
admin_password_file=/secrets/admin.pass
ca_file=/secrets/ca.crt
migrations=/src/crates/oo-jobs/migrations
EOF
chmod 600 "$SEC/jw.conf"
printf '%s\n' "$SEC" > "$PHASE/secrets-path.txt"
wc -c "$SEC/admin.pass" "$SEC/runtime.pass" | awk '{print $1, $2}' > "$PHASE/secret-sizes.txt"

docker network create \
  --label oo.project="$PROJECT" \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.fixed="$FIXED_SOURCE" \
  --label oo.kind=network \
  "$NET" >/dev/null
for vol in "$PG_VOL:postgres" "$CARGO_VOL:cargo" "$TARGET_VOL:target"; do
  name=${vol%%:*}
  kind=${vol##*:}
  docker volume create \
    --label oo.project="$PROJECT" \
    --label oo.owner="$OWNER" \
    --label oo.session="$RUN_ID" \
    --label oo.fixed="$FIXED_SOURCE" \
    --label oo.kind="$kind" \
    "$name" >/dev/null
done

docker run -d --name "$PG_NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add DAC_OVERRIDE \
  --cap-add FOWNER \
  --cap-add SETUID \
  --cap-add SETGID \
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias "$PG_HOST" \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${SRC}/tools/job-worker/ops,target=/fixture,readonly" \
  --mount "type=bind,source=${HERE},target=/oracle,readonly" \
  --label oo.project="$PROJECT" \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.fixed="$FIXED_SOURCE" \
  --label oo.kind=postgres \
  --entrypoint /fixture/pg-wrap.sh \
  "$PG_IMAGE" > "$PHASE/pg.id"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$PG_NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
printf '%s\n' "$ready" > "$PHASE/pg-ready.txt"
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  printf '%s\n' 1 > "$PHASE/pg.exit"
  exit 1
fi
printf '%s\n' 0 > "$PHASE/pg.exit"
docker exec -u postgres "$PG_NAME" psql -U oo_fixture_admin -d postgres -v ON_ERROR_STOP=1 -c "SELECT version();" > "$PHASE/pg-version.txt"
docker exec -u postgres "$PG_NAME" psql -U oo_fixture_admin -d postgres -c "SHOW max_connections;" -c "SHOW shared_buffers;" > "$PHASE/pg-settings.txt"
docker inspect "$PG_NAME" --format '{{json .HostConfig.PortBindings}}' > "$PHASE/pg-ports.json"
docker inspect "$PG_NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PHASE/pg-caps.json"
docker exec "$PG_NAME" sh -c 'for p in /proc/[0-9]*; do tr "\0" " " < "$p/cmdline"; echo; done' > "$PHASE/pg-cmdline.txt" || true
grep -q 'max_connections=128' "$PHASE/pg-cmdline.txt"
mem=$(docker inspect "$PG_NAME" --format '{{.HostConfig.Memory}}')
priv=$(docker inspect "$PG_NAME" --format '{{.HostConfig.Privileged}}')
if [ "$mem" != "1073741824" ] || [ "$priv" != "false" ]; then
  echo "postgres limits were not 1GiB and unprivileged" >&2
  exit 1
fi
if grep -q 'HostPort' "$PHASE/pg-ports.json"; then
  echo "postgres published a host port" >&2
  exit 1
fi

run_rust() {
  local script=$1 tag=$2
  local caller_errexit=0
  case $- in
    *e*) caller_errexit=1 ;;
  esac
  printf '%s\n' "docker run --name ${RUST_NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /evidence/ops/${script}" > "$PHASE/invocation-${tag}.txt"
  set +e
  docker run --name "$RUST_NAME" \
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
    -e OO_JW_CONFIG=/secrets/jw.conf \
    -e OO_PHASE_DIR=/evidence/linux/run \
    --mount "type=bind,source=${SRC},target=/src,readonly" \
    --mount "type=bind,source=${EVIDENCE},target=/evidence" \
    --mount "type=bind,source=${SEC},target=/secrets,readonly" \
    --tmpfs /tmp:rw,nosuid,nodev,noexec,size=1g \
    --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
    --mount "type=volume,source=${TARGET_VOL},target=/target" \
    --label oo.project="$PROJECT" \
    --label oo.owner="$OWNER" \
    --label oo.session="$RUN_ID" \
    --label oo.fixed="$FIXED_SOURCE" \
    --label oo.kind=rust-builder \
    --label oo.toolchain=1.94.0 \
    --label oo.platform=linux/arm64 \
    "$RUST_IMAGE" \
    bash "/evidence/ops/${script}" > "$PHASE/console-${tag}.log" 2>&1
  local run_exit=$?
  if [ "$caller_errexit" -eq 1 ]; then
    set -e
  else
    set +e
  fi
  {
    echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$RUST_NAME")"
    echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$RUST_NAME")"
    echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$RUST_NAME")"
    echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$RUST_NAME")"
    echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$RUST_NAME")"
    echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$RUST_NAME")"
    echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$RUST_NAME")"
  } > "$PHASE/hostconfig-${tag}.txt"
  docker rm "$RUST_NAME" > "$PHASE/container-removed-${tag}.txt"
  printf '%s\n' "$run_exit" > "$PHASE/docker-run-${tag}.exit"
  return "$run_exit"
}

set +e
run_rust inside-build.sh build
build_code=$?
docker exec -u 0 "$PG_NAME" bash /oracle/probe-before.sh > "$PHASE/probe-before.txt" 2> "$PHASE/probe-before.err"
printf '%s\n' "$?" > "$PHASE/probe-before.exit"
run_rust inside-serve.sh serve
serve_code=$?
docker exec -u 0 "$PG_NAME" bash /oracle/probe-after.sh > "$PHASE/probe-after.txt" 2> "$PHASE/probe-after.err"
printf '%s\n' "$?" > "$PHASE/probe-after.exit"
set -e
if [ "$build_code" -ne 0 ]; then
  printf '%s\n' "$build_code" > "$PHASE/launch.exit"
  exit "$build_code"
fi
if [ "$serve_code" -ne 0 ]; then
  printf '%s\n' "$serve_code" > "$PHASE/launch.exit"
  exit "$serve_code"
fi
if [ "$(cat "$PHASE/probe-before.exit")" != "0" ] || [ "$(cat "$PHASE/probe-after.exit")" != "0" ]; then
  printf '%s\n' 1 > "$PHASE/launch.exit"
  exit 1
fi
printf '%s\n' 0 > "$PHASE/launch.exit"
