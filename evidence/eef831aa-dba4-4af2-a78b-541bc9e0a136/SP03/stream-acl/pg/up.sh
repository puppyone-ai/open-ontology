#!/bin/bash
# One owned PostgreSQL 17.10. Secrets stay under /tmp. No host port. No privileged mode.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a136
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP03/stream-acl/pg
SEC=/tmp/oo-a136-${RUN_ID}/secrets
IMAGE=sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a136-eef831aa-net
NAME=oo-a136-pg
LOGVOL=oo-a136-eef831aa-pglog
DATAVOL=oo-a136-eef831aa-pgdata

mkdir -p "$PG"
chmod 755 "$PG/init.sh" "$PG/pg-wrap.sh" "$PG/up.sh"
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$PG/image.txt"
grep -q 'arch=arm64' "$PG/image.txt"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.project=open-ontology \
    --label oo.owner=a136 \
    --label oo.session="${RUN_ID}" \
    "$NET" > "$PG/network.txt"
fi
own_pg=$(docker ps -q --filter label=oo.owner=a136 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own_pg" > "$PG/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$PG/up.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.Status}}' | grep -q .; then
  echo "pg container name already exists" >&2
  docker ps -a --filter "name=^/${NAME}$" --format '{{.Names}} {{.Status}}' >> "$PG/capacity.txt"
  printf '%s\n' 2 > "$PG/up.exit"
  exit 2
fi

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/super.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl rand -hex 24 > "$SEC/writer.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 30 -subj "/CN=oo-a136-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a136-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a136-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 30 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 700 "$SEC"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$PG/server-san.txt"

docker volume inspect "$LOGVOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a136 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres-log \
  "$LOGVOL" >/dev/null
docker volume inspect "$DATAVOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a136 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres-data \
  "$DATAVOL" >/dev/null

docker run -d --name "$NAME" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --cap-add CHOWN \
  --cap-add DAC_OVERRIDE \
  --cap-add FOWNER \
  --cap-add SETUID \
  --cap-add SETGID \
  --memory 512m \
  --memory-swap 512m \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias oo-a136-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=oo_a136 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${LOGVOL},target=/pglog" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${PG},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a136 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=stream-acl \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" >/dev/null

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$NAME" pg_isready -U postgres -d oo_a136 >/dev/null 2>&1; then
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
set +e
docker exec -u postgres "$NAME" bash /fixture/init.sh > "$PG/init.stdout" 2> "$PG/init.stderr"
init_exit=$?
set -e
printf '%s\n' "$init_exit" > "$PG/init.exit"
if [ "$init_exit" -ne 0 ]; then
  printf '%s\n' 1 > "$PG/up.exit"
  exit 1
fi
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$PG/pg-inspect-safe.txt"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$PG/pg-ports.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "$PG/pg-cap-drop.json"
python3 - <<PY
import json
from pathlib import Path
sec = Path("$SEC")
pointer = {
    "host": "oo-a136-pg",
    "port": 5432,
    "database": "oo_a136",
    "reader_user": "oo_a131_reader",
    "writer_user": "oo_a131_writer",
    "super_user": "postgres",
    "password_dir": "/secrets",
    "ca_file": "/secrets/ca.crt",
    "log_file": "/pglog/postgresql.log",
}
(sec / "pointer.json").write_text(json.dumps(pointer) + "\n")
(sec / "pointer.json").chmod(0o600)
Path("$PG/pointer-keys.txt").write_text("\n".join(pointer.keys()) + "\n")
PY
printf '%s\n' 0 > "$PG/up.exit"
echo "pg_up name=$NAME"
