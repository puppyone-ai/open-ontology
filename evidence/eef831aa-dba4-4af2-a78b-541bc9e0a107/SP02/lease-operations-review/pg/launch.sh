#!/bin/bash
# One owned PostgreSQL 17.10 fixture. Trust auth, no password on argv, no host port.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a107
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP02/lease-operations-review
NAME=${RUN_ID}-pg
NET=${RUN_ID}-net
VOL=${RUN_ID}-pgdata

mkdir -p "$EVIDENCE/pg"
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a107 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=fixture-net \
  "$NET" >/dev/null
docker volume inspect "$VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a107 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  "$VOL" >/dev/null

running=$(docker ps --filter label=oo.owner=a107 --filter label=oo.kind=postgres --format '{{.ID}}' || true)
if [ -n "$running" ]; then
  echo "own postgres already running" | tee "$EVIDENCE/pg/capacity.txt"
  exit 0
fi
pg_n=$(docker ps --filter label=oo.kind=postgres --format '{{.ID}}' | grep -c . || true)
echo "postgres_fixtures=${pg_n} own_take=1 rust_compilers_not_counted_here=1" | tee "$EVIDENCE/pg/capacity.txt"
if [ "${pg_n}" -ge 1 ]; then
  echo "deferred: a postgres fixture is already running" >&2
  exit 3
fi

docker run -d \
  --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add DAC_OVERRIDE \
  --cap-add FOWNER \
  --cap-add SETGID \
  --cap-add SETUID \
  --memory 2g \
  --pids-limit 512 \
  --shm-size 256m \
  --network "$NET" \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=postgres \
  -e LANG=C \
  --mount "type=volume,source=${VOL},target=/var/lib/postgresql/data" \
  --label oo.project=open-ontology \
  --label oo.owner=a107 \
  --label oo.session="${RUN_ID}" \
  --label oo.task=lease-operations-review-pg \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp02-w01-b1 \
  --label oo.kind=postgres \
  "$IMAGE" \
  postgres -c max_connections=200 -c superuser_reserved_connections=5 -c listen_addresses='*' \
  > "$EVIDENCE/pg/docker-run.id" 

for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  if docker exec "$NAME" pg_isready -U postgres -d postgres >/dev/null 2>&1; then
    echo ready > "$EVIDENCE/pg/ready.txt"
    docker exec "$NAME" psql -U postgres -d postgres -Atc 'SHOW server_version' > "$EVIDENCE/pg/version.txt"
    docker inspect "$NAME" --format 'status={{.State.Status}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$EVIDENCE/pg/inspect-safe.txt"
    docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "$EVIDENCE/pg/cap-drop.json"
    docker inspect "$NAME" --format '{{json .HostConfig.CapAdd}}' > "$EVIDENCE/pg/cap-add.json"
    docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/pg/ports.json"
    printf '%s\n' 'POSTGRES_HOST_AUTH_METHOD' 'POSTGRES_USER' 'POSTGRES_DB' 'LANG' > "$EVIDENCE/pg/env-keys.txt"
    echo 'password_on_argv=no auth=trust_fixture_only host_port=none' >> "$EVIDENCE/pg/env-keys.txt"
    exit 0
  fi
  sleep 2
done
echo 'postgres did not become ready' >&2
docker inspect "$NAME" --format 'status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}}' > "$EVIDENCE/pg/inspect-safe.txt" || true
exit 1
