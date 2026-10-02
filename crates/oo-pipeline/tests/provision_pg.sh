#!/bin/bash
# Owned PostgreSQL 17.10 fixture for target-fence tests.
# No image pull, no host port, no privileged mode, no host network.
# Tests connect with docker exec to 127.0.0.1 inside the container.
# The official entrypoint needs CHOWN, DAC_OVERRIDE, FOWNER, SETGID, and SETUID
# after cap-drop ALL so it can chown the data directory and drop to postgres.
set -euo pipefail
umask 077

RUN_ID=${RUN_ID:-$(uuidgen | tr '[:upper:]' '[:lower:]')}
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
ROOT=/tmp/oo-a072-pg-${RUN_ID}
NAME=oo-a072-pg1710-${RUN_ID}
NET=oo-a072-pg-${RUN_ID}-net
VOL=oo-a072-pg-${RUN_ID}-data
SRC=$(cd "$(dirname "$0")/../../.." && pwd)
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a072/SP04/target-fence-receipts/pg

mkdir -p "$EVIDENCE" "$ROOT/secrets"
chmod 700 "$ROOT" "$ROOT/secrets"
openssl rand -hex 24 > "$ROOT/secrets/admin_password"
chmod 600 "$ROOT/secrets/admin_password"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  exit 1
fi

own=$(docker ps --filter label=oo.owner=a072 --filter label=oo.kind=postgres --format '{{.Names}}')
if [ -n "$own" ]; then
  echo "this task already has a postgres fixture: $own" >&2
  exit 3
fi

docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a072 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a072 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  "$NET"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a072 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a072 \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  "$VOL" >/dev/null

docker run -d \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add DAC_OVERRIDE \
  --cap-add FOWNER \
  --cap-add SETGID \
  --cap-add SETUID \
  --memory 1g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,nodev,size=256m \
  --mount "type=volume,source=${VOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${ROOT}/secrets,target=/run/secrets,readonly" \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_PASSWORD_FILE=/run/secrets/admin_password \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --label oo.project=open-ontology \
  --label oo.owner=a072 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a072 \
  --label oo.task=target-fence-pg \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp04-target-fence-receipts \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  "$IMAGE" \
  -c listen_addresses='*' \
  -c max_connections=50 \
  -c shared_preload_libraries='' \
  > "$EVIDENCE/container.id"

printf '%s\n' "$RUN_ID" > "$EVIDENCE/run-id.txt"
printf '%s\n' "$NAME" > "$EVIDENCE/container-name.txt"
printf '%s\n' "$IMAGE" > "$EVIDENCE/image-ref.txt"
echo "provision_started name=$NAME"
