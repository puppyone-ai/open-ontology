#!/bin/bash
# One owned PostgreSQL 17.10. Secrets stay under the private tmp directory.
# No host port. No privileged mode. Does not pull. Does not prune.
# Origin: a274 pg/up.sh. Alias and SAN are oo-a305-pg for this demo.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a305
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/deploy/trusted-demo/pg
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP11/trusted-demo/pg
SEC=/tmp/oo-a305-${RUN_ID}/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a305-eef831aa-net
NAME=oo-a305-pg
DATAVOL=oo-a305-eef831aa-pgdata

mkdir -p "$EVIDENCE"
chmod 755 "$PG/pg-wrap.sh" "$PG/up.sh"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$EVIDENCE/up.exit"
  exit 1
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/image.txt"

if docker network inspect "$NET" >/dev/null 2>&1; then
  net_owner=$(docker network inspect "$NET" --format '{{index .Labels "oo.owner"}}')
  if [ "$net_owner" != "a305" ]; then
    echo "network name is not owned by a305" >&2
    printf '%s\n' 2 > "$EVIDENCE/up.exit"
    exit 2
  fi
else
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a305 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" >/dev/null
fi

wait_ready() {
  ready=0
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
    if docker exec -u postgres "$NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 2
  done
  if [ "$ready" -ne 1 ]; then
    echo "postgres did not become ready" >&2
    docker logs "$NAME" > "$EVIDENCE/postgres-start.log" 2>&1 || true
    printf '%s\n' 1 > "$EVIDENCE/up.exit"
    exit 1
  fi
  docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}}' > "$EVIDENCE/inspect.txt"
  docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/ports.json"
  docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$EVIDENCE/caps.json"
  printf '%s\n' 0 > "$EVIDENCE/up.exit"
  echo "postgres_ready name=$NAME alias=oo-a305-pg"
}

if docker container inspect "$NAME" >/dev/null 2>&1; then
  owner=$(docker inspect "$NAME" --format '{{index .Config.Labels "oo.owner"}}')
  if [ "$owner" != "a305" ]; then
    echo "pg container name is not owned by a305" >&2
    printf '%s\n' 2 > "$EVIDENCE/up.exit"
    exit 2
  fi
  running=$(docker inspect "$NAME" --format '{{.State.Running}}')
  if [ "$running" != "true" ]; then
    docker start "$NAME" >/dev/null
  fi
  wait_ready
  exit 0
fi

own_pg=$(docker ps -q --filter label=oo.owner=a305 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own_pg" > "$EVIDENCE/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$EVIDENCE/up.exit"
  exit 3
fi

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
if [ ! -s "$SEC/admin.pass" ]; then
  openssl rand -hex 24 > "$SEC/admin.pass"
  openssl rand -hex 24 > "$SEC/reader.pass"
  openssl rand -hex 24 > "$SEC/writer.pass"
  openssl rand -hex 24 > "$SEC/runtime.pass"
  openssl rand -hex 24 > "$SEC/login.pass"
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a305-ca" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a305-pg" >/dev/null 2>&1
  cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a305-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
  chmod 600 "$SEC"/*
  chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
  chmod 600 "$SEC/server.key" "$SEC/ca.key" "$SEC"/*.pass
fi
for need in admin.pass reader.pass writer.pass runtime.pass login.pass ca.crt ca.key server.crt server.key; do
  if [ ! -s "$SEC/$need" ]; then
    echo "secret material incomplete: $need" >&2
    printf '%s\n' 1 > "$EVIDENCE/up.exit"
    exit 1
  fi
done
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$EVIDENCE/server-san.txt"
printf '%s\n' "$SEC" > "$EVIDENCE/secrets-path.txt"

if docker volume inspect "$DATAVOL" >/dev/null 2>&1; then
  vol_owner=$(docker volume inspect "$DATAVOL" --format '{{index .Labels "oo.owner"}}')
  if [ "$vol_owner" != "a305" ]; then
    echo "pg volume is not owned by a305" >&2
    printf '%s\n' 2 > "$EVIDENCE/up.exit"
    exit 2
  fi
  printf '%s\n' existing > "$EVIDENCE/volume-data.txt"
else
  docker volume create \
    --label oo.project=open-ontology \
    --label oo.owner=a305 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=postgres \
    "$DATAVOL" > "$EVIDENCE/volume-data.txt"
fi

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
  --network-alias oo-a305-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${PG},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a305 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=trusted-demo \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" > "$EVIDENCE/container.id"
wait_ready
