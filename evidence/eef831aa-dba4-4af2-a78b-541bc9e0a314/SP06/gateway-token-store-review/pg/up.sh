#!/bin/bash
# One owned PostgreSQL 17.10. Secrets stay under /tmp. No host port. No privileged mode.
# Recipe origin: a200 pg/up.sh, then a304 pg/up.sh. Changes from a304: UUID a314 names,
# network created here, SAN includes oo-a314-pg and the frozen author alias oo-a304-pg,
# and init creates database oo_a314 beside the frozen name oo_a304.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a314
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP06/gateway-token-store-review/pg
SEC=/tmp/oo-a314-${RUN_ID}/secrets
FIXTURE=/tmp/oo-a314-${RUN_ID}/builder-fixture
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a314-eef831aa-net
NAME=oo-a314-pg
DATAVOL=oo-a314-eef831aa-pgdata

mkdir -p "$PG"
chmod 755 "$PG/pg-wrap.sh" "$PG/up.sh" "$PG/init-roles.sh"
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"
own_pg=$(docker ps -q --filter label=oo.owner=a314 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own_pg" > "$PG/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$PG/up.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.Status}}' | grep -q .; then
  echo "pg container name already exists" >&2
  printf '%s\n' 2 > "$PG/up.exit"
  exit 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a314 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null
docker network inspect --format '{{.Id}} driver={{.Driver}} internal={{.Internal}}' "$NET" > "$PG/network.txt"

umask 077
mkdir -p "$SEC" "$FIXTURE"
chmod 700 "$SEC" "$FIXTURE"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/token-admin.pass"
openssl rand -hex 24 > "$SEC/token-runtime.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a314-ca" >/dev/null 2>&1
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/wrong.key" -out "$SEC/wrong.crt" -days 7 -subj "/CN=oo-a314-wrong" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a314-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a314-pg,DNS:oo-a304-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt" "$SEC/wrong.crt"
cp "$SEC/ca.crt" "$SEC/wrong.crt" "$SEC/admin.pass" "$SEC/token-admin.pass" "$SEC/token-runtime.pass" "$FIXTURE/"
chmod 644 "$FIXTURE/ca.crt" "$FIXTURE/wrong.crt"
chmod 600 "$FIXTURE/admin.pass" "$FIXTURE/token-admin.pass" "$FIXTURE/token-runtime.pass"
find "$FIXTURE" -type f -print | sort > "$PG/builder-fixture-files.txt"
if find "$FIXTURE" -type f \( -name '*.key' -o -name 'server.crt' \) | grep -q .; then
  echo "builder fixture contains a private or server key" >&2
  printf '%s\n' 2 > "$PG/up.exit"
  exit 2
fi
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$PG/server-san.txt"

docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a314 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  "$DATAVOL" > "$PG/volume-data.txt"

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
  --network-alias oo-a314-pg \
  --network-alias oo-a304-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=oo_a304 \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${PG},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a314 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=gateway-token-store-review \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" > "$PG/container.id"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$NAME" pg_isready -U oo_fixture_admin -d oo_a304 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  docker logs "$NAME" > "$PG/postgres-start.log" 2>&1 || true
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker exec "$NAME" /fixture/init-roles.sh > "$PG/roles.txt" 2> "$PG/roles-err.txt" || {
  sed -E "s/PASSWORD '[0-9a-fA-F]+'/PASSWORD 'redacted'/g" "$PG/roles-err.txt" > "$PG/roles-err-redacted.txt" || true
  rm -f "$PG/roles-err.txt"
  echo "role creation failed" >&2
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
}
rm -f "$PG/roles-err.txt"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$PG/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$PG/ports.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$PG/caps.json"
printf '%s\n' 0 > "$PG/up.exit"
echo "postgres_ready name=$NAME"
