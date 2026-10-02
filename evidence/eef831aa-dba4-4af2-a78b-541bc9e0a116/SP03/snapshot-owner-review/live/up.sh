#!/bin/bash
# One private PostgreSQL 17.10 fixture. Secrets stay under /tmp and are not copied into evidence.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a116
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP03/snapshot-owner-review
LIVE=${EVIDENCE}/live
SEC=/tmp/oo-a116-${RUN_ID}/secrets
IMAGE=sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=${RUN_ID}-net
NAME=${RUN_ID}-pg
VOL=${RUN_ID}-pglog

docker image inspect "$IMAGE" >/dev/null
docker network inspect "$NET" >/dev/null
if docker ps --filter "name=^/${NAME}$" --filter status=running --format '{{.ID}}' | grep -q .; then
  echo "pg already running" >&2
  exit 1
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.ID}}' | grep -q .; then
  echo "pg container exists and is not running; refusing to replace it" >&2
  exit 1
fi

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/super.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl rand -hex 24 > "$SEC/writer.pass"
openssl rand -hex 24 > "$SEC/guc.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 30 -subj "/CN=oo-a116-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a116-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a116-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 30 -extfile "$SEC/san.cnf" >/dev/null 2>&1
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/wrong-ca.key" -out "$SEC/wrong-ca.crt" -days 30 -subj "/CN=oo-a116-wrong-ca" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 700 "$SEC"

docker volume inspect "$VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a116 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
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
  --memory 512m \
  --memory-swap 512m \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias oo-a116-pg \
  --network-alias oo-a116-wrong \
  -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=revdb \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${VOL},target=/pglog" \
  --mount "type=bind,source=${LIVE},target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a116 \
  --label oo.session="${RUN_ID}" \
  --label oo.run_id="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.scope=sp03-r1-review \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" >/dev/null

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$NAME" pg_isready -U postgres -d revdb >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "${LIVE}/pg-inspect-safe.txt" || true
  exit 1
fi
docker exec -u postgres "$NAME" bash /fixture/init.sh
mkdir -p "${LIVE}/expected"
docker cp "$NAME":/tmp/a116-cut.txt "${LIVE}/expected/cut.txt"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}}' > "${LIVE}/pg-inspect-safe.txt"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "${LIVE}/pg-ports.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}}' > "${LIVE}/pg-cap-drop.json"
cat > "${SEC}/pointer.txt" <<EOF
host oo-a116-pg
wrong_host oo-a116-wrong
port 5432
database revdb
reader_user oo_rev_reader
writer_user oo_rev_writer
guc_user oo_rev_guc
admin_user postgres
reader_password_file /secrets/reader.pass
writer_password_file /secrets/writer.pass
guc_password_file /secrets/guc.pass
admin_password_file /secrets/super.pass
ca_file /secrets/ca.crt
wrong_ca_file /secrets/wrong-ca.crt
log_file /pglog/postgresql.log
cut_file /expected/cut.txt
EOF
chmod 600 "${SEC}/pointer.txt"
cp "${SEC}/pointer.txt" "${LIVE}/pointer.txt"
cat > "${LIVE}/resources.txt" <<EOF
pg_container=${NAME}
pg_volume=${VOL}
network=${NET}
secret_dir=${SEC}
image=${IMAGE}
alias=oo-a116-pg
wrong_alias=oo-a116-wrong
database=revdb
privileged=no
host_port=none
EOF
echo "pg_ready"
