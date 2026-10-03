#!/bin/bash
# One private PostgreSQL 17.10. Secrets stay under /tmp. No host port.
set -euo pipefail
SRC=/Users/redux/puppy-one/open-ontology
EVIDENCE=${SRC}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a133/SP03/init-tail
ROOT=/tmp/oo-a133-eef831aa
SEC=${ROOT}/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a133-eef831aa-net
NAME=oo-a133-init-tail-pg
VOL=oo-a133-eef831aa-pglog
POINTER=${SEC}/pointer.json

mkdir -p "$EVIDENCE/pg"
own_pg=$(docker ps -q --filter label=oo.owner=a133 --filter label=oo.kind=postgres | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own_pg" > "$EVIDENCE/pg/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: own postgres is already running" >&2
  printf '%s\n' 3 > "$EVIDENCE/pg/up.exit"
  exit 3
fi

docker image inspect --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$IMAGE" > "$EVIDENCE/pg/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/pg/image.txt"
if ! docker network inspect "$NET" >/dev/null 2>&1; then
  echo "proof network is missing" >&2
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
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 2 -subj "/CN=a133-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=a133pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:a133pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -out "$SEC/server.crt" -days 2 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 700 "$SEC"
rm -f "$SEC/server.csr" "$SEC/ca.key" "$SEC/san.cnf"

docker volume inspect "$VOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a133 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a133 \
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
  --network-alias a133pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_a133_admin \
  -e POSTGRES_DB=proof \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${VOL},target=/pglog" \
  --mount "type=bind,source=${EVIDENCE}/pg,target=/fixture,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a133 \
  --label oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a133 \
  --label oo.kind=postgres \
  --label oo.task=init-tail-pg \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" >/dev/null

ready=0
for _ in $(seq 1 45); do
  if docker exec -u postgres "$NAME" pg_isready -U oo_a133_admin -d proof >/dev/null 2>&1; then
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

cat > "$POINTER" <<'EOF'
{
  "host": "a133pg",
  "port": 5432,
  "database": "proof",
  "admin_user": "oo_a133_admin",
  "reader_user": "oo_a133_reader",
  "admin_password_file": "/secrets/admin.pass",
  "reader_password_file": "/secrets/reader.pass",
  "ca_file": "/secrets/ca.crt",
  "log_file": "/pglog/postgresql.log"
}
EOF
chmod 600 "$POINTER"
python3 - <<'PY'
import json
p="/tmp/oo-a133-eef831aa/secrets/pointer.json"
data=json.load(open(p))
blob="\n".join(open(path).read() for path in (
    "/tmp/oo-a133-eef831aa/secrets/admin.pass",
    "/tmp/oo-a133-eef831aa/secrets/reader.pass"))
text=json.dumps(data)
assert "admin.pass" in text and "BEGIN" not in text
for secret in blob.split():
    assert secret not in text
print("pointer_has_no_secret=yes")
PY
printf '%s\n' 0 > "$EVIDENCE/pg/up.exit"
echo "PG_READY"
