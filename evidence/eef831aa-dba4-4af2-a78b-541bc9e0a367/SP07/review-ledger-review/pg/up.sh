#!/bin/bash
# One owned PostgreSQL 17.10. Secrets stay under the private tmp directory.
# No host port. No privileged mode. Does not pull. Does not prune.
# pg-wrap.sh is the a274 file, unchanged.
# Origin flags: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a274/SP11/pg-onboarding-review/pg/up.sh
# SHA-256 ec1dd1087fddfebfe22c89ee21677ed6c81eba75f7ba4e6a55b2d867daa02c76
# Reviewed copy: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a361/SP14/pg-backup-restore-review/pg/up.sh
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a367
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP07/review-ledger-review/pg
HARNESS=${PG}
ROOT=/tmp/oo-a367-${RUN_ID}
SEC=${ROOT}/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a367-eef831aa-net
NAME=oo-a367-pg
DATAVOL=oo-a367-eef831aa-pgdata
WRAP_SHA=d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb

mkdir -p "$PG"
chmod 755 "$HARNESS/pg-wrap.sh" "$HARNESS/up.sh"
got=$(shasum -a 256 "$HARNESS/pg-wrap.sh" | awk '{print $1}')
if [ "$got" != "$WRAP_SHA" ]; then
  echo "pg-wrap.sh is not the a274 bytes" >&2
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is not present locally; not pulling" >&2
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"

own_pg=$(docker ps -aq --filter label=oo.owner=a367 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres=%s\n' "$own_pg" > "$PG/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a postgres container" >&2
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
    --label oo.owner=a367 \
    --label oo.session="${RUN_ID}" \
    --label oo.kind=network \
    "$NET" >/dev/null
fi

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/super.pass"
openssl rand -hex 24 > "$SEC/ledger-admin.pass"
openssl rand -hex 24 > "$SEC/runtime-a358.pass"
openssl rand -hex 24 > "$SEC/runtime-a367.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a367-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=pg.a367.internal" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:pg.a367.internal
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
chmod 600 "$SEC/server.key" "$SEC/ca.key"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$PG/server-san.txt"
shasum -a 256 "$SEC/ca.crt" | awk '{print $1}' > "$PG/ca.sha256"
printf '%s\n' "$SEC" > "$PG/secrets-path.txt"
stat -f '%Lp' "$SEC" > "$PG/password-mode.txt" 2>/dev/null || stat -c '%a' "$SEC" > "$PG/password-mode.txt"

docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a367 \
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
  --network-alias pg.a367.internal \
  -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
  -e POSTGRES_USER=oo_a367_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${HARNESS},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a367 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=review-ledger-review \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" > "$PG/container.id"

ready=0
i=0
while [ "$i" -lt 30 ]; do
  i=$((i + 1))
  if docker exec -u postgres "$NAME" pg_isready -U oo_a367_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
docker inspect "$NAME" --format 'oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano={{.HostConfig.NanoCpus}}' > "$PG/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.CapAdd}} {{json .HostConfig.CapDrop}}' > "$PG/caps.json"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$PG/ports.json"
docker network inspect "$NET" --format '{{.Id}}' > "$PG/network.id"
if [ "$ready" -ne 1 ]; then
  docker logs "$NAME" > "$PG/server.txt" 2>&1 || true
  echo "postgres did not become ready" >&2
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker exec -u postgres "$NAME" postgres --version > "$PG/server.txt"
docker exec -u postgres "$NAME" pg_isready -U oo_a367_admin -d postgres >> "$PG/server.txt"

admin_pw=$(cat "$SEC/ledger-admin.pass")
runtime_a358=$(cat "$SEC/runtime-a358.pass")
runtime_a367=$(cat "$SEC/runtime-a367.pass")
sql="$SEC/bootstrap.sql"
cat > "$sql" <<EOF
DO \$\$ BEGIN
  CREATE ROLE oo_review_ledger_admin LOGIN PASSWORD '${admin_pw}';
EXCEPTION WHEN duplicate_object THEN NULL;
END \$\$;
DO \$\$ BEGIN
  CREATE ROLE oo_review_ledger_runtime NOLOGIN;
EXCEPTION WHEN duplicate_object THEN NULL;
END \$\$;
DO \$\$ BEGIN
  CREATE ROLE oo_a358_runtime LOGIN PASSWORD '${runtime_a358}';
EXCEPTION WHEN duplicate_object THEN NULL;
END \$\$;
DO \$\$ BEGIN
  CREATE ROLE oo_a367_runtime LOGIN PASSWORD '${runtime_a367}';
EXCEPTION WHEN duplicate_object THEN NULL;
END \$\$;
GRANT oo_review_ledger_runtime TO oo_a358_runtime WITH INHERIT TRUE;
GRANT oo_review_ledger_runtime TO oo_a367_runtime WITH INHERIT TRUE;
EOF
docker exec -i -u postgres "$NAME" psql -U oo_a367_admin -d postgres -v ON_ERROR_STOP=1 < "$sql" > "$PG/bootstrap.out"
rm -f "$sql"
docker exec -u postgres "$NAME" psql -U oo_a367_admin -d postgres -v ON_ERROR_STOP=1 \
  -c 'CREATE DATABASE oo_a358_review OWNER oo_review_ledger_admin' \
  -c 'CREATE DATABASE oo_a367_oracle OWNER oo_review_ledger_admin' > "$PG/database.out"
printf '%s\n' 0 > "$PG/up.exit"
