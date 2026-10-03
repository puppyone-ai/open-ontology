#!/bin/bash
# One private PostgreSQL 17.10. Secrets stay under /tmp. No host port.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a135/SP03/init-tail-review
ROOT=/tmp/oo-a135-eef831aa
SEC=${ROOT}/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a135-eef831aa-net
NAME=oo-a135-init-tail-pg
LOGVOL=oo-a135-eef831aa-pglog
DATAVOL=oo-a135-eef831aa-pgdata

mkdir -p "$EVIDENCE/pg"
chmod 755 "$EVIDENCE/pg/pg-wrap.sh" "$EVIDENCE/pg/init.sh" "$EVIDENCE/pg/up.sh"
own_pg=$(docker ps -q --filter label=oo.owner=a135 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own_pg" > "$EVIDENCE/pg/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$EVIDENCE/pg/up.exit"
  exit 3
fi
docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/pg/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/pg/image.txt"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "review network is missing" >&2
  printf '%s\n' 4 > "$EVIDENCE/pg/up.exit"
  exit 4
fi
if docker container inspect "$NAME" >/dev/null 2>&1; then
  prior=$(docker inspect -f '{{.State.Status}}' "$NAME")
  if [ "$prior" = "running" ]; then
    echo "deferred: named postgres is already running" >&2
    printf '%s\n' 3 > "$EVIDENCE/pg/up.exit"
    exit 3
  fi
  docker rm "$NAME" >/dev/null
fi

umask 077
rm -rf "$SEC"
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/super.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl rand -hex 24 > "$SEC/writer.pass"
openssl rand -hex 24 > "$SEC/admin.pass"
cp "$SEC/super.pass" "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/a135-reader.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 2 -subj "/CN=oo-a135-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a135-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a135-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -out "$SEC/server.crt" -days 2 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 700 "$SEC"
rm -f "$SEC/server.csr" "$SEC/ca.key" "$SEC/san.cnf"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$EVIDENCE/pg/server-san.txt"

cat > "$SEC/stream-pointer.json" <<'EOF'
{
  "host": "oo-a135-pg",
  "port": 5432,
  "database": "oo_a135",
  "reader_user": "oo_a131_reader",
  "writer_user": "oo_a131_writer",
  "super_user": "postgres",
  "password_dir": "/secrets",
  "ca_file": "/secrets/ca.crt",
  "log_file": "/pglog/postgresql.log"
}
EOF
cat > "$SEC/tls-pointer.json" <<'EOF'
{
  "host": "oo-a135-pg",
  "port": 5432,
  "database": "oo_a135",
  "admin_user": "postgres",
  "reader_user": "oo_a135_reader",
  "admin_password_file": "/secrets/admin.pass",
  "reader_password_file": "/secrets/a135-reader.pass",
  "ca_file": "/secrets/ca.crt",
  "log_file": "/pglog/postgresql.log"
}
EOF
chmod 600 "$SEC"/*.json
python3 - <<'PY'
import json
root = "/tmp/oo-a135-eef831aa/secrets"
secrets = []
for name in ("super.pass", "reader.pass", "writer.pass", "admin.pass", "a135-reader.pass"):
    secrets.append(open(f"{root}/{name}").read().strip())
for name in ("stream-pointer.json", "tls-pointer.json"):
    text = open(f"{root}/{name}").read()
    json.loads(text)
    assert "BEGIN" not in text
    for secret in secrets:
        assert secret not in text
print("pointers_have_no_secret=yes")
PY

docker volume inspect "$LOGVOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a135 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a135 --label oo.kind=postgres-log \
  "$LOGVOL" >/dev/null
docker volume inspect "$DATAVOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology --label oo.owner=a135 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a135 --label oo.kind=postgres-data \
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
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 256 \
  --network "$NET" \
  --network-alias oo-a135-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=oo_a135 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${LOGVOL},target=/pglog" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${EVIDENCE}/pg,target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a135 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a135 \
  --label oo.kind=postgres \
  --label oo.task=init-tail-review-pg \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" >/dev/null

ready=0
for _ in $(seq 1 45); do
  if docker exec -u postgres "$NAME" pg_isready -U postgres -d oo_a135 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
docker inspect "$NAME" --format 'ExitCode={{.State.ExitCode}}
Status={{.State.Status}}
OOMKilled={{.State.OOMKilled}}
Privileged={{.HostConfig.Privileged}}
NetworkMode={{.HostConfig.NetworkMode}}
Memory={{.HostConfig.Memory}}
CapDrop={{json .HostConfig.CapDrop}}
CapAdd={{json .HostConfig.CapAdd}}
PortBindings={{json .HostConfig.PortBindings}}' > "$EVIDENCE/pg/hostconfig.txt" || true
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  docker logs "$NAME" > "$EVIDENCE/pg/postgres-not-ready.log" 2>&1 || true
  printf '%s\n' 1 > "$EVIDENCE/pg/up.exit"
  exit 1
fi
docker exec -u postgres "$NAME" bash /fixture/init.sh > "$EVIDENCE/pg/init.stdout" 2> "$EVIDENCE/pg/init.stderr"
printf '%s\n' $? > "$EVIDENCE/pg/init.exit"
if [ "$(cat "$EVIDENCE/pg/init.exit")" -ne 0 ]; then
  printf '%s\n' 1 > "$EVIDENCE/pg/up.exit"
  exit 1
fi
version=$(docker exec -u postgres "$NAME" psql -At -c "SHOW server_version")
printf '%s\n' "$version" > "$EVIDENCE/pg/server-version.txt"
printf '%s\n' 0 > "$EVIDENCE/pg/up.exit"
echo "PG_READY"
