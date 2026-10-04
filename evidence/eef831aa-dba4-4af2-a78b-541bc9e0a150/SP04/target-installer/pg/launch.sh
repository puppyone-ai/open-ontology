#!/bin/bash
# One private PostgreSQL 17.10 for the a150 target installer probe.
# No image pull, no host port, no privileged mode, no host network.
set -euo pipefail
umask 077

SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a150/SP04/target-installer/pg
ROOT=/tmp/oo-a150-eef831aa-dba4-4af2-a78b-541bc9e0a150
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NAME=oo-a150-pg
NET=oo-a150-eef831aa-net
VOL=oo-a150-eef831aa-pg

mkdir -p "$EVIDENCE" "$ROOT/pg-secrets"
chmod 700 "$ROOT" "$ROOT/pg-secrets"
if [ ! -s "$ROOT/pg-secrets/admin_password" ]; then
  openssl rand -hex 24 > "$ROOT/pg-secrets/admin_password"
fi
chmod 600 "$ROOT/pg-secrets/admin_password"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  exit 1
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"

own=$(docker ps -q --filter label=oo.owner=a150 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own" > "$EVIDENCE/capacity.txt"
if [ "$own" -ge 1 ]; then
  echo "this task already has a running postgres fixture" >&2
  exit 3
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  echo "named postgres container already exists" >&2
  exit 2
fi
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "a150 network is absent" >&2
  exit 2
fi
docker volume inspect "$VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a150 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a150 \
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
  --mount "type=bind,source=${ROOT}/pg-secrets,target=/run/secrets,readonly" \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_PASSWORD_FILE=/run/secrets/admin_password \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --label oo.project=open-ontology \
  --label oo.owner=a150 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a150 \
  --label oo.task=target-installer-pg \
  --label oo.kind=postgres \
  --label oo.platform=linux/arm64 \
  "$IMAGE" \
  -c listen_addresses='*' \
  -c max_connections=50 \
  -c shared_preload_libraries='' \
  > "$EVIDENCE/container.id"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec "$NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
printf '%s\n' "$ready" > "$EVIDENCE/ready.txt"
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  docker logs "$NAME" > "$EVIDENCE/postgres.log" 2>&1 || true
  exit 1
fi
docker exec "$NAME" postgres --version > "$EVIDENCE/postgres-version.txt"
echo "postgres_ready name=$NAME"
