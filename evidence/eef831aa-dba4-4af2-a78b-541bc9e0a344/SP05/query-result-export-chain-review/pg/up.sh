#!/bin/bash
# Private PostgreSQL 17.10 for the a344 export chain review. 1 GiB, 1 CPU.
# Origin: a333 actual-pg-chain pg/up.sh
# SHA-256 9ddc412a92f5c96dbcaaf8871d284f0a4fcccf5f05be330396b395bd2ab89d78
# which reused a340, origin a200 up.sh 96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45
# Changes: owner a344, private /tmp secrets, alias oo-a344-pg, no published port.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a344
REPO=/Users/redux/puppy-one/open-ontology
PG=${REPO}/evidence/${RUN_ID}/SP05/query-result-export-chain-review/pg
OUT=${PG}
SEC=/tmp/oo-a344-eef831aa-dba4-4af2-a78b-541bc9e0a344/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a344-eef831aa-net
NAME=oo-a344-pg
DATAVOL=oo-a344-eef831aa-pgdata

mkdir -p "$OUT"
chmod 755 "$PG/pg-wrap.sh" "$PG/up.sh"
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$OUT/image.txt"
grep -q 'arch=arm64' "$OUT/image.txt"

own_pg=$(docker ps -q --filter label=oo.owner=a344 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s memory=1g cpus=1\n' "$own_pg" > "$OUT/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$OUT/up.exit"
  exit 3
fi
if docker ps -a --filter "name=^/${NAME}$" --format '{{.Status}}' | grep -q .; then
  echo "pg container name already exists" >&2
  printf '%s\n' 2 > "$OUT/up.exit"
  exit 2
fi
docker network inspect "$NET" >/dev/null 2>&1 || docker network create \
  --label oo.project=open-ontology \
  --label oo.owner=a344 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=network \
  "$NET" >/dev/null

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl rand -hex 24 > "$SEC/runtime.pass"
openssl rand -hex 24 > "$SEC/login.pass"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 7 -subj "/CN=oo-a344-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-a344-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-a344-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 7 -extfile "$SEC/san.cnf" >/dev/null 2>&1
chmod 600 "$SEC"/*
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
chmod 600 "$SEC/server.key" "$SEC/ca.key" "$SEC/admin.pass" "$SEC/reader.pass" "$SEC/runtime.pass" "$SEC/login.pass"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$OUT/server-san.txt"
printf '%s\n' "$SEC" > "$OUT/secrets-path.txt"

if docker volume inspect "$DATAVOL" >/dev/null 2>&1; then
  echo "postgres data volume already exists" >&2
  printf '%s\n' 2 > "$OUT/up.exit"
  exit 2
fi
docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a344 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  "$DATAVOL" > "$OUT/volume-data.txt"
sync
sleep 2
ls "$SEC/ca.crt" "$SEC/server.crt" "$SEC/admin.pass" >/dev/null

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
  --network-alias oo-a344-pg \
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
  --label oo.owner=a344 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=query-result-export-chain-review \
  --entrypoint /fixture/pg-wrap.sh \
  "$IMAGE" > "$OUT/container.id"

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
  docker logs "$NAME" > "$OUT/postgres-start.log" 2>&1 || true
  printf '%s\n' 1 > "$OUT/up.exit"
  exit 1
fi
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} nano_cpus={{.HostConfig.NanoCpus}}' > "$OUT/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$OUT/ports.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$OUT/caps.json"
docker exec -u postgres "$NAME" psql -U oo_fixture_admin -d postgres -At -c "SHOW server_version" > "$OUT/server-version.txt"
printf '%s\n' 0 > "$OUT/up.exit"
echo "postgres_ready name=$NAME"
