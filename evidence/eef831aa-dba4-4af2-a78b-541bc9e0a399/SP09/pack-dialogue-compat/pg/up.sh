#!/bin/bash
# One private PostgreSQL 17.10 for UUID a399. Password files stay under /tmp.
# pg-wrap.sh is the reviewed a389 file, mounted read-only and not modified.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a399
REPO=/Users/redux/puppy-one/open-ontology
BASE=${REPO}/evidence/${RUN_ID}/SP09/pack-dialogue-compat
OUT=${BASE}/pg
SEC=/tmp/oo-a399-${RUN_ID}/secrets
IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a399-eef831aa-net
NAME=oo-a399-pg
DATAVOL=oo-a399-eef831aa-pgdata
WRAP=${REPO}/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a389/SP12/product-benchmark/pg/pg-wrap.sh

mkdir -p "$OUT"
docker image inspect "$IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$OUT/image.txt"
grep -q 'arch=arm64' "$OUT/image.txt"
own_pg=$(docker ps -q --filter label=oo.owner=a399 --filter label=oo.kind=postgres --filter status=running | sed '/^$/d' | wc -l | tr -d ' ')
printf 'own_postgres_running=%s\n' "$own_pg" > "$OUT/capacity.txt"
if [ "$own_pg" -ge 1 ]; then
  echo "deferred: this task already has a running postgres" >&2
  printf '%s\n' 3 > "$OUT/up.exit"
  exit 3
fi
docker network inspect "$NET" >/dev/null

umask 077
mkdir -p "$SEC"
chmod 700 "$SEC"
openssl rand -hex 24 > "$SEC/admin.pass"
openssl rand -hex 24 > "$SEC/login.pass"
cp "${REPO}/contracts/sdk/json-query/fixtures/tls/ca.pem" "$SEC/ca.crt"
cp "${REPO}/contracts/sdk/json-query/fixtures/tls/localhost.pem" "$SEC/server.crt"
cp "${REPO}/contracts/sdk/json-query/fixtures/tls/localhost.key" "$SEC/server.key"
chmod 600 "$SEC/admin.pass" "$SEC/login.pass" "$SEC/server.key"
chmod 644 "$SEC/ca.crt" "$SEC/server.crt"
openssl x509 -in "$SEC/server.crt" -noout -subject -ext subjectAltName > "$OUT/server-san.txt"
find "$OUT" -type f \( -name '*.key' -o -name '*.pass' -o -name 'server.crt' \) | grep -q . && {
  echo "evidence pg dir contains secret material" >&2
  exit 2
}

docker volume inspect "$DATAVOL" >/dev/null 2>&1 || docker volume create \
  --label oo.project=open-ontology \
  --label oo.owner=a399 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  "$DATAVOL" > "$OUT/volume-data.txt"

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
  --network-alias oo-a399-pg \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=oo_a399 \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC},target=/secrets,readonly" \
  --mount "type=bind,source=${SEC},target=/tls-in,readonly" \
  --mount "type=volume,source=${DATAVOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${WRAP},target=/fixture/pg-wrap.sh,readonly" \
  --mount "type=bind,source=${BASE}/pg/load.sh,target=/fixture/load.sh,readonly" \
  --mount "type=bind,source=${REPO}/crates/oo-pipeline/migrations,target=/migrations,readonly" \
  --label oo.project=open-ontology \
  --label oo.owner=a399 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=postgres \
  --label oo.task=pack-dialogue-compat \
  --entrypoint /bin/bash \
  "$IMAGE" /fixture/pg-wrap.sh > "$OUT/container.id"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec -u postgres "$NAME" pg_isready -U oo_fixture_admin -d oo_a399 >/dev/null 2>&1; then
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
docker exec "$NAME" /fixture/load.sh > "$OUT/load.stdout" 2> "$OUT/load.stderr" || {
  sed -E 's/PASSWORD '\''[^'\'']*'\''/PASSWORD '\''redacted'\''/g' "$OUT/load.stderr" > "$OUT/load.stderr.redacted" || true
  rm -f "$OUT/load.stderr"
  printf '%s\n' 1 > "$OUT/up.exit"
  exit 1
}
rm -f "$OUT/load.stderr"
docker inspect "$NAME" --format 'id={{.Id}} status={{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}}' > "$OUT/inspect.txt"
docker inspect "$NAME" --format '{{json .HostConfig.PortBindings}}' > "$OUT/ports.json"
docker inspect "$NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$OUT/caps.json"
printf '%s\n' 0 > "$OUT/up.exit"
