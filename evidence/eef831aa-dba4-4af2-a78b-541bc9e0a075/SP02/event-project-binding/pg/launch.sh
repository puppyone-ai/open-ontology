#!/bin/bash
# Owned PostgreSQL 17.10 fixture. No host port. Finite caps for the image entrypoint.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a075
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
SRC=/Users/redux/puppy-one/open-ontology
ROOT=/tmp/oo-a075-${RUN_ID}
EVIDENCE=${SRC}/evidence/${RUN_ID}/SP02/event-project-binding/pg
NAME=oo-a075-pg1710-jobs
NET=oo-a075-pg-net
VOL=oo-a075-pg-data
CRATE=${SRC}/crates/oo-jobs

mkdir -p "$EVIDENCE" "$ROOT"
if docker ps -a --filter name="^/${NAME}$" --format '{{.Names}}' | grep -qx "$NAME"; then
  echo "own postgres container already exists" >&2
  exit 3
fi
own=$(docker ps --filter label=oo.owner=a075 --filter label=oo.kind=pg-fixture --format '{{.Names}}')
if [ -n "$own" ]; then
  echo "own postgres fixture already running" >&2
  exit 3
fi

openssl rand -hex 24 > "$ROOT/pg.password"
chmod 600 "$ROOT/pg.password"
docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a075 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=pg-fixture \
  "$NET"
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a075 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=pg-fixture \
  "$VOL" >/dev/null

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
  --cpus 2 \
  --pids-limit 512 \
  --network "$NET" \
  --shm-size 256m \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_PASSWORD_FILE=/run/secrets/pgpw \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  --mount "type=bind,source=${ROOT}/pg.password,target=/run/secrets/pgpw,readonly" \
  --mount "type=bind,source=${CRATE},target=/fixture,readonly" \
  --mount "type=volume,source=${VOL},target=/var/lib/postgresql/data" \
  --label oo.project=open-ontology \
  --label oo.owner=a075 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.scope=sp02-event-project-binding \
  --label oo.platform=linux/arm64 \
  --label oo.kind=pg-fixture \
  "$IMAGE" >/dev/null

ready=0
for _ in $(seq 1 60); do
  if docker exec "$NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
printf '%s\n' "$ready" > "$EVIDENCE/ready.txt"
if [ "$ready" -ne 1 ]; then
  docker logs "$NAME" > "$EVIDENCE/postgres-start.log" 2>&1 || true
  echo "postgres did not become ready" >&2
  exit 1
fi
docker exec "$NAME" postgres --version > "$EVIDENCE/version.txt"
docker inspect "$NAME" --format 'name={{.Name}} image={{.Image}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} pids={{.HostConfig.PidsLimit}} oom={{.State.OOMKilled}}' > "$EVIDENCE/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "$EVIDENCE/cap-drop.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapAdd}}' > "$EVIDENCE/cap-add.json"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/ports.json"
docker inspect "$NAME" --format '{{json .Config.Env}}' | python3 -c 'import json,sys; keys=sorted(item.split("=",1)[0] for item in json.load(sys.stdin)); print("\n".join(keys))' > "$EVIDENCE/env-keys.txt"
echo "postgres_ready name=$NAME"
