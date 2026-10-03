#!/bin/bash
# One private PostgreSQL 17.10 for a130. Secrets stay under /tmp. No host port.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a130
REPO=/Users/redux/puppy-one/open-ontology
LIVE=${REPO}/evidence/${RUN_ID}/SP03/reader-primitives-review/pg
SEC=/tmp/oo-a130-${RUN_ID}/secrets
IMAGE=sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=${RUN_ID}-net
NAME=${RUN_ID}-pg
VOL=${RUN_ID}-pglog
POINTER=${SEC}/pointer.json

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$LIVE/image.txt"
grep -q 'arch=arm64' "$LIVE/image.txt"
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a130 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null
own_pg=$(docker ps -q --filter label=oo.owner=a130 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own_pg" > "$LIVE/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$LIVE/up.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.ID}}' | grep -q .; then
  echo "pg container name already exists" >&2
  printf '%s\n' 2 > "$LIVE/up.exit"
  exit 2
fi

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/super.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl rand -hex 24 > "$SEC/writer.pass"
openssl rand -hex 24 > "$SEC/guc.pass"
openssl rand -hex 24 > "$SEC/lot-reader.pass"
openssl rand -hex 24 > "$SEC/lot-writer.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 30 -subj "/CN=a130-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=a130pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:a130pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 30 -extfile "$SEC/san.cnf" >/dev/null 2>&1
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/wrong-ca.key" -out "$SEC/wrong-ca.crt" -days 30 -subj "/CN=a130-wrong-ca" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 700 "$SEC"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$LIVE/server-san.txt"

docker volume inspect "$VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a130 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  "$VOL" >/dev/null

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
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias a130pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=snapdb \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${VOL},target=/pglog" \
  --mount "type=bind,source=${LIVE},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a130 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=reader-primitives-review \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" >/dev/null

ready=0
for _ in $(seq 1 45); do
  if docker exec -u postgres "$NAME" pg_isready -U postgres -d snapdb >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "$LIVE/pg-inspect-safe.txt" || true
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$LIVE/pg-ports.json" || true
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$LIVE/pg-cap-drop.json" || true
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  docker logs "$NAME" > "$LIVE/postgres-start.log" 2>&1 || true
  printf '%s\n' 1 > "$LIVE/up.exit"
  exit 1
fi

init_tmp=$(mktemp)
init_err=$(mktemp)
chmod 600 "$init_tmp" "$init_err"
set +e
docker exec -u postgres "$NAME" bash /fixture/init.sh >"$init_tmp" 2>"$init_err"
init_ec=$?
set -e
leak=0
for file in "$SEC"/super.pass "$SEC"/reader.pass "$SEC"/writer.pass "$SEC"/guc.pass "$SEC"/lot-reader.pass "$SEC"/lot-writer.pass; do
  if grep -F -q -f "$file" "$init_tmp" || grep -F -q -f "$file" "$init_err"; then
    leak=1
  fi
done
if [ "$leak" -ne 0 ]; then
  rm -f "$init_tmp" "$init_err"
  echo "init_output_leak=yes" > "$LIVE/init-status.txt"
  printf '%s\n' 1 > "$LIVE/up.exit"
  exit 1
fi
cp "$init_tmp" "$LIVE/init.out"
cp "$init_err" "$LIVE/init.err"
rm -f "$init_tmp" "$init_err"
printf '%s\n' "$init_ec" > "$LIVE/init.exit"
if [ "$init_ec" -ne 0 ]; then
  echo "init failed" >&2
  printf '%s\n' 1 > "$LIVE/up.exit"
  exit 1
fi

cat > "$POINTER" <<'EOF'
{
  "host": "a130pg",
  "port": 5432,
  "database": "snapdb",
  "reader_user": "oo_snap_reader",
  "writer_user": "oo_snap_writer",
  "guc_user": "oo_snap_guc",
  "super_user": "postgres",
  "password_dir": "/secrets",
  "ca_file": "/secrets/ca.crt",
  "wrong_ca_file": "/secrets/wrong-ca.crt",
  "log_file": "/pglog/postgresql.log",
  "lot_reader_user": "lot_reader",
  "lot_writer_user": "lot_writer",
  "lot_reader_password_file": "lot-reader.pass",
  "lot_writer_password_file": "lot-writer.pass",
  "super_password_file": "super.pass"
}
EOF
chmod 600 "$POINTER"
cat > "$SEC/portal-pointer.txt" <<'EOF'
host a130pg
port 5432
database snapdb
ca_file /secrets/ca.crt
reader_user oo_a127_reader
reader_password_file /secrets/reader.pass
writer_user oo_a127_writer
writer_password_file /secrets/writer.pass
admin_user postgres
admin_password_file /secrets/super.pass
log_file /pglog/postgresql.log
EOF
chmod 600 "$SEC/portal-pointer.txt"
awk '{print $1}' "$SEC/portal-pointer.txt" > "$LIVE/pointer-keys.txt"
printf '%s\n' "$POINTER" > "$LIVE/pointer-path.txt"
cat > "$LIVE/resources.txt" <<EOF
pg_container=${NAME}
pg_volume=${VOL}
network=${NET}
secret_dir=${SEC}
pointer=${POINTER}
image=${IMAGE}
alias=a130pg
database=snapdb
privileged=no
host_port=none
log_statement_during_role_create=off
EOF
printf '%s\n' 0 > "$LIVE/up.exit"
echo "pg_ready"
