#!/bin/bash
# One private PostgreSQL 17.10 and one linux/arm64 Rust 1.94 builder.
# Origin shape: deploy/trusted-demo/linux/launch.sh
# origin_commit 10b78993641c88154c75c22219c79c1566e2cecd
# origin_sha256 520c4604dd98be92a7e6c32455cff05cbd856f5ba7c6a8260d8ea8a7cc2b58d5
# Does not pull, prune, publish a host port, or mount the Docker socket or host home.
set -eu
set -o pipefail

MODE=${1:-}
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
OWNER=a390
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a390
PROJECT=open-ontology
NET=oo-a390-jw
PG_NAME=oo-a390-jw-pg
RUST_NAME=oo-a390-jw-rust
PG_VOL=oo-a390-jw-pgdata
CARGO_VOL=oo-a390-jw-cargo
TARGET_VOL=oo-a390-jw-target
PG_HOST=pg
EVIDENCE="$REPO/evidence/$RUN_ID/SP02/job-worker"
SEC=/tmp/oo-a390-$RUN_ID/secrets
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
FIXED_SOURCE=$(git -C "$REPO" rev-parse HEAD)

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

require_missing_or_ours() {
  local kind=$1 name=$2 state
  state=$(object_state "$kind" "$name")
  case "$state" in
    missing|ours) ;;
    foreign)
      echo "foreign labels on $kind $name" >&2
      exit 2
      ;;
  esac
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

preclean_test() {
  local name owner session
  for name in "$RUST_NAME" "$PG_NAME" oo-a390-jw-smoke oo-a390-jw-smoke2; do
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
  remove_ours volume "$PG_VOL"
  remove_ours network "$NET"
  if [ -d "$SEC" ]; then
    rm -rf "$SEC"
    echo "removed secrets dir"
  fi
  echo "kept volumes $CARGO_VOL $TARGET_VOL"
}

cleanup_owned() {
  local name owner session
  for name in "$RUST_NAME" "$PG_NAME" oo-a390-jw-smoke oo-a390-jw-smoke2; do
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
  remove_ours volume "$PG_VOL"
  remove_ours volume "$CARGO_VOL"
  remove_ours volume "$TARGET_VOL"
  remove_ours network "$NET"
  if [ -d "$SEC" ]; then
    rm -rf "$SEC"
    echo "removed secrets dir"
  fi
}

next_phase() {
  local base n
  base=$1
  if [ ! -d "$EVIDENCE/linux/$base" ]; then
    printf '%s\n' "$EVIDENCE/linux/$base"
    return
  fi
  n=1
  while [ -d "$EVIDENCE/linux/${base}-attempt-$n" ]; do
    n=$((n + 1))
  done
  printf '%s\n' "$EVIDENCE/linux/${base}-attempt-$n"
}

write_recipe() {
  local dest=$1
  {
    echo "fixed_source=$FIXED_SOURCE"
    echo "launch_origin=deploy/trusted-demo/linux/launch.sh"
    echo "launch_commit=10b78993641c88154c75c22219c79c1566e2cecd"
    echo "launch_sha256=520c4604dd98be92a7e6c32455cff05cbd856f5ba7c6a8260d8ea8a7cc2b58d5"
    echo "up_origin=deploy/trusted-demo/pg/up.sh"
    echo "up_sha256=904aed7ed10156576252da4be7ad828832aefe4628d15f67c1436f475cad9bff"
    echo "pg_wrap_origin=deploy/trusted-demo/pg/pg-wrap.sh"
    echo "pg_wrap_sha256=05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a"
    echo "pg_wrap_deviation=max_connections=128"
    echo "tls_origin=tools/session-store/src/tls.rs"
    echo "tls_sha256=65598bc49a24de7ad3a32d576a57345194b1a6da6fb0fd93f037836810cddf61"
    echo "lock_origin=tools/session-store/Cargo.lock"
    echo "lock_sha256=ad43022b2fd98a158781df1f7dbb928b857683e54871a4210f3360bfff525f9a"
    echo "secrets_path=$SEC"
  } > "$dest"
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
PHASE=$(next_phase run)
mkdir -p "$PHASE"
write_recipe "$EVIDENCE/recipe-origin.txt"
cp "$EVIDENCE/recipe-origin.txt" "$PHASE/recipe-origin.txt"
if ! docker info >/dev/null 2>&1; then
  echo "docker is not available" >&2
  printf '%s\n' 1 > "$PHASE/docker-info.exit"
  exit 1
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

preclean_test > "$PHASE/preclean.txt"
require_missing_or_ours network "$NET"
require_missing_or_ours volume "$PG_VOL"
require_missing_or_ours volume "$CARGO_VOL"
require_missing_or_ours volume "$TARGET_VOL"
require_missing_or_ours container "$PG_NAME"
require_missing_or_ours container "$RUST_NAME"

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
  state=$(object_state volume "$name")
  case "$state" in
    missing)
      docker volume create \
        --label oo.project="$PROJECT" \
        --label oo.owner="$OWNER" \
        --label oo.session="$RUN_ID" \
        --label oo.fixed="$FIXED_SOURCE" \
        --label oo.kind="$kind" \
        "$name" >/dev/null
      ;;
    ours) echo "reuse volume $name" ;;
    *)
      echo "foreign volume $name" >&2
      exit 2
      ;;
  esac
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
  --mount "type=bind,source=${HERE},target=/fixture,readonly" \
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
docker logs "$PG_NAME" > "$PHASE/postgres.log" 2>&1 || true
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  printf '%s\n' 1 > "$PHASE/pg.exit"
  exit 1
fi
printf '%s\n' 0 > "$PHASE/pg.exit"
docker inspect "$PG_NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}}' > "$PHASE/pg-inspect.txt"
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

rel=${PHASE#"$EVIDENCE"}
printf '%s\n' "docker run --name ${RUST_NAME} --platform linux/arm64 --pull never --network ${NET} --cap-drop ALL --memory 6g --cpus 1 image=pinned-rust-1.94 bash /src/tools/job-worker/ops/inside.sh" > "$PHASE/invocation.txt"
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
  -e OO_JW_EVIDENCE=/evidence \
  -e OO_PHASE_DIR=/evidence"$rel" \
  --mount "type=bind,source=${REPO},target=/src,readonly" \
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
  bash /src/tools/job-worker/ops/inside.sh > "$PHASE/console.log" 2>&1
run_exit=$?
set -e
{
  echo "ExitCode=$(docker inspect -f '{{.State.ExitCode}}' "$RUST_NAME")"
  echo "OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$RUST_NAME")"
  echo "Privileged=$(docker inspect -f '{{.HostConfig.Privileged}}' "$RUST_NAME")"
  echo "NetworkMode=$(docker inspect -f '{{.HostConfig.NetworkMode}}' "$RUST_NAME")"
  echo "CapDrop=$(docker inspect -f '{{json .HostConfig.CapDrop}}' "$RUST_NAME")"
  echo "Memory=$(docker inspect -f '{{.HostConfig.Memory}}' "$RUST_NAME")"
  echo "NanoCpus=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$RUST_NAME")"
  echo "PidsLimit=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$RUST_NAME")"
} > "$PHASE/hostconfig.txt"
docker inspect -f '{{json .HostConfig.Tmpfs}}' "$RUST_NAME" > "$PHASE/tmpfs.txt"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Source}} {{.Destination}} rw={{.RW}}{{println}}{{end}}' "$RUST_NAME" > "$PHASE/mounts.txt"
if grep -F 'docker.sock' "$PHASE/mounts.txt" >/dev/null; then
  echo "docker socket is mounted" >&2
  run_exit=1
fi
docker rm "$RUST_NAME" > "$PHASE/container-removed.txt"
printf '%s\n' "$run_exit" > "$PHASE/docker-run.exit"
exit "$run_exit"
