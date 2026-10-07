#!/bin/bash
# One owned PostgreSQL 17.10. Secrets stay under the private tmp directory.
# No host port. No privileged mode. Does not pull. Does not prune.
# Origin: a274 pg/up.sh. Names come from the deployment manifest.
set -euo pipefail
PG=$(cd "$(dirname "$0")" && pwd)
DEMO=$(cd "$PG/.." && pwd)
REPO=$(cd "$DEMO/../.." && pwd)
# shellcheck source=../linux/manifest.sh
. "$DEMO/linux/manifest.sh"
MODE=${1:-}
case "$MODE" in
  create|reuse) ;;
  *)
    echo "usage: up.sh create|reuse" >&2
    exit 2
    ;;
esac
load_deployment "${OO_DEMO_MANIFEST:-}"
bind_resources "$REPO"
EVIDENCE_PG=${EVIDENCE}/pg
SEC=${ROOT}/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb

mkdir -p "$EVIDENCE_PG"
chmod 755 "$PG/pg-wrap.sh" "$PG/up.sh"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE_PG/up.exit"
  exit 1
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE_PG/image.txt"
grep -q 'arch=arm64' "$EVIDENCE_PG/image.txt"

wait_ready() {
  local ready
  ready=0
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
    if docker exec -u postgres "$PG_NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 2
  done
  if [ "$ready" -ne 1 ]; then
    echo "postgres did not become ready" >&2
    docker logs "$PG_NAME" > "$EVIDENCE_PG/postgres-start.log" 2>&1 || true
    printf '%s\n' 1 > "$EVIDENCE_PG/up.exit"
    exit 1
  fi
  docker inspect "$PG_NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}}' > "$EVIDENCE_PG/inspect.txt"
  docker inspect "$PG_NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE_PG/ports.json"
  docker inspect "$PG_NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$EVIDENCE_PG/caps.json"
  printf '%s\n' 0 > "$EVIDENCE_PG/up.exit"
  echo "postgres_ready name=$PG_NAME alias=$PG_HOST"
}

run_pg() {
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
    --mount "type=bind,source=${PG},target=/fixture,readonly" \
    --label oo.project="$PROJECT" \
    --label oo.owner="$OWNER" \
    --label oo.session="$RUN_ID" \
    --label oo.fixed="$FIXED_SOURCE" \
    --label oo.kind=postgres \
    --label oo.task=trusted-demo \
    --entrypoint /fixture/pg-wrap.sh \
    "$IMAGE" > "$EVIDENCE_PG/container.id"
}

ensure_network() {
  local state
  state=$(object_state network "$NET")
  case "$state" in
    missing)
      if [ "$MODE" != "create" ]; then
        echo "owned network is absent" >&2
        printf '%s\n' 2 > "$EVIDENCE_PG/up.exit"
        exit 2
      fi
      docker network create \
        --label oo.project="$PROJECT" \
        --label oo.owner="$OWNER" \
        --label oo.session="$RUN_ID" \
        --label oo.fixed="$FIXED_SOURCE" \
        --label oo.kind=network \
        "$NET" >/dev/null
      ;;
    ours)
      if [ "$MODE" = "create" ]; then
        echo "resource collision on network $NET; refusing to overwrite" >&2
        printf '%s\n' 2 > "$EVIDENCE_PG/up.exit"
        exit 2
      fi
      ;;
    foreign)
      echo "foreign labels on network $NET; refusing" >&2
      printf '%s\n' 2 > "$EVIDENCE_PG/up.exit"
      exit 2
      ;;
  esac
}

ensure_volume() {
  local state
  state=$(object_state volume "$PG_VOL")
  case "$state" in
    missing)
      if [ "$MODE" != "create" ]; then
        echo "owned pg volume is absent" >&2
        printf '%s\n' 2 > "$EVIDENCE_PG/up.exit"
        exit 2
      fi
      docker volume create \
        --label oo.project="$PROJECT" \
        --label oo.owner="$OWNER" \
        --label oo.session="$RUN_ID" \
        --label oo.fixed="$FIXED_SOURCE" \
        --label oo.kind=postgres \
        "$PG_VOL" > "$EVIDENCE_PG/volume-data.txt"
      ;;
    ours)
      if [ "$MODE" = "create" ]; then
        echo "resource collision on volume $PG_VOL; refusing to overwrite" >&2
        printf '%s\n' 2 > "$EVIDENCE_PG/up.exit"
        exit 2
      fi
      printf '%s\n' existing > "$EVIDENCE_PG/volume-data.txt"
      ;;
    foreign)
      echo "foreign labels on volume $PG_VOL; refusing" >&2
      printf '%s\n' 2 > "$EVIDENCE_PG/up.exit"
      exit 2
      ;;
  esac
}

mint_secrets() {
  local need
  if [ "$MODE" = "create" ]; then
    if [ -e "$ROOT" ]; then
      echo "private directory already exists; refusing to overwrite" >&2
      printf '%s\n' 2 > "$EVIDENCE_PG/up.exit"
      exit 2
    fi
    umask 077
    mkdir -p "$SEC"
    chmod 700 "$SEC"
  else
    if [ ! -d "$SEC" ]; then
      echo "private secrets are absent" >&2
      printf '%s\n' 2 > "$EVIDENCE_PG/up.exit"
      exit 2
    fi
  fi
  if [ "$MODE" = "create" ]; then
    openssl rand -hex 24 > "$SEC/admin.pass"
    openssl rand -hex 24 > "$SEC/reader.pass"
    openssl rand -hex 24 > "$SEC/writer.pass"
    openssl rand -hex 24 > "$SEC/runtime.pass"
    openssl rand -hex 24 > "$SEC/login.pass"
    openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-${OWNER}-ca" >/dev/null 2>&1
    openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=${PG_HOST}" >/dev/null 2>&1
    printf 'basicConstraints=CA:FALSE\nsubjectAltName=DNS:%s\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' "$PG_HOST" > "$SEC/san.cnf"
    openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
    chmod 600 "$SEC"/*
    chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
    chmod 600 "$SEC/server.key" "$SEC/ca.key" "$SEC"/*.pass
  fi
  for need in admin.pass reader.pass writer.pass runtime.pass login.pass ca.crt ca.key server.crt server.key; do
    if [ ! -s "$SEC/$need" ]; then
      echo "secret material incomplete: $need" >&2
      printf '%s\n' 1 > "$EVIDENCE_PG/up.exit"
      exit 1
    fi
  done
  openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$EVIDENCE_PG/server-san.txt"
  printf '%s\n' "$SEC" > "$EVIDENCE_PG/secrets-path.txt"
}

ensure_network
ensure_volume
state=$(object_state container "$PG_NAME")
case "$state" in
  ours)
    if [ "$MODE" = "create" ]; then
      echo "resource collision on container $PG_NAME; refusing to overwrite" >&2
      printf '%s\n' 2 > "$EVIDENCE_PG/up.exit"
      exit 2
    fi
    running=$(docker inspect "$PG_NAME" --format '{{.State.Running}}')
    if [ "$running" != "true" ]; then
      docker start "$PG_NAME" >/dev/null
    fi
    wait_ready
    exit 0
    ;;
  foreign)
    echo "foreign labels on container $PG_NAME; refusing" >&2
    printf '%s\n' 2 > "$EVIDENCE_PG/up.exit"
    exit 2
    ;;
  missing) ;;
esac

if [ "$MODE" = "create" ]; then
  own_pg=$(docker ps -q --filter label=oo.owner="$OWNER" --filter label=oo.session="$RUN_ID" --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
  printf 'own_postgres_running=%s\n' "$own_pg" > "$EVIDENCE_PG/capacity.txt"
  if [ "$own_pg" -ge 1 ]; then
    echo "deferred: this task already has a running postgres" >&2
    printf '%s\n' 3 > "$EVIDENCE_PG/up.exit"
    exit 3
  fi
fi

mint_secrets
run_pg
wait_ready
