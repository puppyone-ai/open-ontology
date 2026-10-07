#!/bin/bash
# One owned PostgreSQL 17.10. Secrets stay under the private tmp directory.
# No host port. No privileged mode. Does not pull. Does not prune.
# pg-wrap.sh is the a274 reviewed file, byte-identical.
# Origin flags: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/pg/up.sh
# SHA-256 ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76
# Later reviewed copy of the same flags: a361 pg/up.sh
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a366
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP08/release-coordinator-review/pg
HARNESS=${PG}
ROOT=/tmp/oo-a366-${RUN_ID}
SEC=${ROOT}/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a366-eef831aa-net
NAME=oo-a366-pg
DATAVOL=oo-a366-eef831aa-pgdata

mkdir -p "$PG"
chmod 755 "$HARNESS/pg-wrap.sh" "$HARNESS/up.sh"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"

own_pg=$(docker ps -q --filter label=oo.owner=a366 --filter label=oo.kind=postgres --filter status=running | wc -l | tr -d ' ')
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
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a366 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" >/dev/null
fi
docker network inspect "$NET" --format '{{.Id}}' > "$PG/network.id"

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/super.pass"
openssl rand -hex 24 > "$SEC/release-admin.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a366-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a366-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a366-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
rm -f "$SEC/server.csr" "$SEC/ca.srl" "$SEC/san.cnf"
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
chmod 600 "$SEC/server.key" "$SEC/ca.key" "$SEC"/*.pass
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$PG/server-san.txt"
shasum -a 256 "$SEC/ca.crt" "$SEC/server.crt" | awk '{print $1}' > "$PG/cert-public.sha256"
printf '%s\n' "$SEC" > "$PG/secrets-path.txt"
stat -f '%Lp' "$SEC/release-admin.pass" > "$PG/password-mode.txt" 2>/dev/null || stat -c '%a' "$SEC/release-admin.pass" > "$PG/password-mode.txt"

docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a366 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  "$DATAVOL" > "$PG/volume.id"

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
  --network-alias oo-a366-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
  -e POSTGRES_USER=oo_a366_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${HARNESS},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a366 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=release-coordinator-review \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" > "$PG/container.id"

ready=0
i=0
while [ "$i" -lt 60 ]; do
  i=$((i + 1))
  if docker exec -u postgres "$NAME" pg_isready -U oo_a366_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
docker inspect "$NAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$PG/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.CapAdd}} {{json .HostConfig.CapDrop}}' > "$PG/caps.json"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$PG/ports.json"
if [ "$ready" -ne 1 ]; then
  docker logs "$NAME" > "$PG/server.txt" 2>&1 || true
  echo "postgres did not become ready" >&2
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker exec -u postgres "$NAME" pg_isready -U oo_a366_admin -d postgres > "$PG/server.txt"

# Role passwords are read inside the container. They are not written to evidence.
docker exec -i -u postgres "$NAME" bash -s > "$PG/bootstrap.out" <<'EOS'
set -eu
umask 077
admin=$(cat /secrets/release-admin.pass)
reader=$(cat /secrets/reader.pass)
super=$(cat /secrets/super.pass)
sql=/tmp/oo-a366-bootstrap.sql
printf "CREATE ROLE oo_release_admin LOGIN PASSWORD '%s';\n" "$admin" > "$sql"
printf "CREATE ROLE oo_release_reader NOLOGIN;\n" >> "$sql"
printf "CREATE ROLE oo_release_reader_login LOGIN PASSWORD '%s' IN ROLE oo_release_reader;\n" "$reader" >> "$sql"
printf "CREATE DATABASE oo_a366 OWNER oo_release_admin;\n" >> "$sql"
printf "CREATE DATABASE oo_a366_author OWNER oo_release_admin;\n" >> "$sql"
export PGPASSWORD="$super"
psql -U oo_a366_admin -d postgres -v ON_ERROR_STOP=1 -f "$sql"
rm -f "$sql"
unset PGPASSWORD
EOS
printf '%s\n' 0 > "$PG/up.exit"
