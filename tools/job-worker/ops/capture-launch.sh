#!/bin/bash
# Isolated control and target PostgreSQL for DL01-W1-74.
# Two databases on one server is the recorded exception. No source write, no foreign volume delete.
# Does not pull images, publish a host port, or mount the Docker socket.
set -eu
set -o pipefail

MODE=${1:-}
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
OWNER=dl01w1-74
RUN_ID=d101fd4a-3a89-41f6-b7fe-94b4001d5512
PROJECT=open-ontology
NET=oo-dl01w1-74
PG_NAME=oo-dl01w1-74-pg
RUST_NAME=oo-dl01w1-74-rust
PG_VOL=oo-dl01w1-74-pgdata
CARGO_VOL=oo-dl01w1-74-cargo
TARGET_VOL=oo-dl01w1-74-target
PG_HOST=pg
EVIDENCE="$REPO/evidence/$RUN_ID/DL01/capture-jobs-schedule-lease"
SEC=/tmp/oo-dl01w1-74-$RUN_ID/secrets
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
PG_MEM=4294967296
RUST_MEM=17179869184

case "$MODE" in
  prove|down) ;;
  *)
    echo "usage: capture-launch.sh prove|down" >&2
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
  remove_ours container "$RUST_NAME" || true
  remove_ours container "$PG_NAME" || true
  remove_ours volume "$PG_VOL" || true
  remove_ours volume "$CARGO_VOL" || true
  remove_ours volume "$TARGET_VOL" || true
  remove_ours network "$NET" || true
  if [ -d "$SEC" ]; then
    rm -rf "$SEC"
    echo "removed secrets dir"
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

mkdir -p "$EVIDENCE/linux"
PHASE="$EVIDENCE/linux/run"
if [ -d "$PHASE" ]; then
  n=1
  while [ -d "$EVIDENCE/linux/run-attempt-$n" ]; do
    n=$((n + 1))
  done
  PHASE="$EVIDENCE/linux/run-attempt-$n"
fi
mkdir -p "$PHASE"
printf '%s\n' "$PHASE" > "$EVIDENCE/linux/phase-path.txt"

if ! docker info >/dev/null 2>&1; then
  echo "docker is not available" >&2
  printf '%s\n' 1 > "$PHASE/docker-info.exit"
  exit 1
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

for name in "$NET" "$PG_VOL" "$CARGO_VOL" "$TARGET_VOL"; do
  kind=volume
  if [ "$name" = "$NET" ]; then kind=network; fi
  state=$(object_state "$kind" "$name")
  if [ "$state" = "foreign" ]; then
    echo "foreign $kind $name" >&2
    exit 2
  fi
done
for name in "$PG_NAME" "$RUST_NAME"; do
  state=$(object_state container "$name")
  if [ "$state" = "ours" ]; then
    docker rm -f "$name" >/dev/null
  elif [ "$state" = "foreign" ]; then
    echo "foreign container $name" >&2
    exit 2
  fi
done

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/runtime.pass"
openssl rand -hex 24 > "$SEC/pipe.pass"
openssl rand -hex 16 > "$SEC/capture.ref"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-${OWNER}-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=${PG_HOST}" >/dev/null 2>&1
printf 'basicConstraints=CA:FALSE\nsubjectAltName=DNS:%s\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' "$PG_HOST" > "$SEC/san.cnf"
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
cat > "$SEC/jw.conf" <<EOF
host=$PG_HOST
port=5432
database=oo_d101_control
runtime_user=oo_d101_jobs_login
runtime_password_file=/secrets/runtime.pass
admin_user=oo_d101_admin
admin_password_file=/secrets/admin.pass
ca_file=/secrets/ca.crt
migrations=/src/crates/oo-jobs/migrations
target_database=oo_d101_target
target_runtime_user=oo_d101_pipe_login
target_runtime_password_file=/secrets/pipe.pass
pipeline_migrations=/src/crates/oo-pipeline/migrations
capture_root=/src
secret_ref=/secrets/capture.ref
EOF
chmod 600 "$SEC/jw.conf"
{
  echo "databases=oo_d101_control,oo_d101_target"
  echo "exception=two databases on one postgres server"
  echo "source_sql=not_run"
  echo "secrets_dir=$SEC"
  echo "pg_memory=$PG_MEM"
  echo "rust_memory=$RUST_MEM"
  echo "cpu_cap=none"
  echo "cargo_build_jobs=unset"
} > "$PHASE/recipe.txt"

if [ "$(object_state network "$NET")" = "missing" ]; then
  docker network create \
    --subnet 10.74.174.0/24 \
    --label oo.project="$PROJECT" \
    --label oo.owner="$OWNER" \
    --label oo.session="$RUN_ID" \
    --label oo.kind=network \
    "$NET" >/dev/null
fi
for spec in "$PG_VOL:postgres" "$CARGO_VOL:cargo" "$TARGET_VOL:target"; do
  name=${spec%%:*}
  kind=${spec##*:}
  if [ "$(object_state volume "$name")" = "missing" ]; then
    docker volume create \
      --label oo.project="$PROJECT" \
      --label oo.owner="$OWNER" \
      --label oo.session="$RUN_ID" \
      --label oo.kind="$kind" \
      "$name" >/dev/null
  fi
done

echo "starting postgres"
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
  --memory "$PG_MEM" \
  --memory-swap "$PG_MEM" \
  --pids-limit 512 \
  --network "$NET" \
  --network-alias "$PG_HOST" \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_d101_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${HERE},target=/fixture,readonly" \
  --label oo.project="$PROJECT" \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.kind=postgres \
  --entrypoint /fixture/pg-wrap.sh \
  "$PG_IMAGE" > "$PHASE/pg.id"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$PG_NAME" pg_isready -U oo_d101_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
docker logs "$PG_NAME" > "$PHASE/postgres.log" 2>&1 || true
printf '%s\n' "$ready" > "$PHASE/pg-ready.txt"
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  exit 1
fi
docker inspect "$PG_NAME" --format 'oom={{.State.OOMKilled}} memory={{.HostConfig.Memory}} privileged={{.HostConfig.Privileged}}' > "$PHASE/pg-inspect.txt"
echo "postgres ready"

echo "starting rust"
docker run -d --name "$RUST_NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory "$RUST_MEM" \
  --memory-swap "$RUST_MEM" \
  --network "$NET" \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e CARGO_TERM_COLOR=never \
  -e OO_JW_CONFIG=/secrets/jw.conf \
  -e OO_PHASE_DIR=/evidence/linux/$(basename "$PHASE") \
  --mount "type=bind,source=${REPO},target=/src,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --tmpfs /tmp:rw,nosuid,nodev,size=1g \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --label oo.project="$PROJECT" \
  --label oo.owner="$OWNER" \
  --label oo.session="$RUN_ID" \
  --label oo.kind=rust-builder \
  --label oo.toolchain=1.94.0 \
  "$RUST_IMAGE" \
  sleep infinity > "$PHASE/rust.id"

exec_phase() {
  local name=$1
  set +e
  docker exec "$RUST_NAME" bash /src/tools/job-worker/ops/capture-proof.sh "$name" > "$PHASE/$name.console" 2>&1
  local status=$?
  set -e
  printf '%s\n' "$status" > "$PHASE/$name.phase.exit"
  echo "phase=$name exit=$status"
  return "$status"
}

set +e
echo "phase build begin"
exec_phase build
build_status=$?
if [ "$build_status" -eq 0 ]; then
  exec_phase before-restart
  before_status=$?
else
  before_status=skipped
fi
if [ "$before_status" = "0" ]; then
  docker restart "$PG_NAME" > "$PHASE/pg-restart.id"
  ready=0
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    if docker exec -u postgres "$PG_NAME" pg_isready -U oo_d101_admin -d postgres >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 2
  done
  printf '%s\n' "$ready" > "$PHASE/pg-restart-ready.txt"
  if [ "$ready" -eq 1 ]; then
    exec_phase after-restart
    after_status=$?
  else
    after_status=1
    echo "postgres did not return after restart" | tee "$PHASE/after-restart.console"
  fi
else
  after_status=skipped
fi
docker stats --no-stream --format '{{.Name}} mem={{.MemUsage}} cpu={{.CPUPerc}}' "$PG_NAME" "$RUST_NAME" > "$PHASE/docker-stats.txt" || true
docker inspect "$RUST_NAME" --format 'oom={{.State.OOMKilled}} memory={{.HostConfig.Memory}}' > "$PHASE/rust-inspect.txt"
docker inspect "$PG_NAME" --format 'oom={{.State.OOMKilled}} memory={{.HostConfig.Memory}}' > "$PHASE/pg-inspect-end.txt"
{
  echo "build=$build_status"
  echo "before_restart=$before_status"
  echo "after_restart=$after_status"
} > "$PHASE/phases.txt"
set -e
if [ "$build_status" -ne 0 ] || [ "$before_status" != "0" ] || [ "$after_status" != "0" ]; then
  exit 1
fi
exit 0
