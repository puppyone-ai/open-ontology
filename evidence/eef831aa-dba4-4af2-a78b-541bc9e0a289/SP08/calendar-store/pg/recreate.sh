#!/bin/bash
# Replace this task's PostgreSQL data volume after impl-1.
# impl-1 changed the database owner and PostgreSQL dropped oo_calendar_admin from the ACL.
# This removes only oo-a289-pg and oo-a289-eef831aa-pgdata. It does not prune.
# Secrets and certificates under /tmp stay. The init SQL owner is oo_fixture_admin.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a289
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP08/calendar-store/pg
ROOT=/tmp/oo-a289-${RUN_ID}
SEC=${ROOT}/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a289-eef831aa-net
NAME=oo-a289-pg
DATAVOL=oo-a289-eef831aa-pgdata

if [ -f "$PG/recreate.exit" ]; then
  echo "refusing to overwrite pg/recreate.exit" >&2
  exit 2
fi
owner=$(docker inspect -f '{{index .Config.Labels "oo.owner"}}' "$NAME")
test "$owner" = "a289"
sed -i '' 's/OWNER oo_calendar_admin;/OWNER oo_fixture_admin;/' "$SEC/init/01-roles.sql"
grep -q 'OWNER oo_fixture_admin;' "$SEC/init/01-roles.sql"
docker stop "$NAME" > "$PG/recreate-stop.txt"
docker rm "$NAME" > "$PG/recreate-rm-container.txt"
docker volume rm "$DATAVOL" > "$PG/recreate-rm-volume.txt"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a289 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  "$DATAVOL" > "$PG/recreate-volume.txt"
docker run -d --name "$NAME" \
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
  --network-alias oo-a289-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/fixture-admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=bind,source=${SEC}/init,target=/docker-entrypoint-initdb.d,readonly" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${PG},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a289 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=calendar-store \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" > "$PG/recreate-container.id"
ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  docker logs "$NAME" 2>&1 | sed -e "s/PASSWORD '[^']*'/PASSWORD 'redacted'/g" > "$PG/recreate-start.log" || true
  printf '%s\n' 1 > "$PG/recreate.exit"
  exit 1
fi
docker exec -u postgres "$NAME" psql -U oo_fixture_admin -d postgres -tAc "SELECT datacl FROM pg_database WHERE datname = 'oo_a289'" > "$PG/recreate-datacl.txt"
printf '%s\n' 0 > "$PG/recreate.exit"
echo "postgres_recreated name=$NAME"
