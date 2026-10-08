#!/bin/bash
# One private PostgreSQL 17.10 and one Rust 1.94 builder.
# Does not pull, prune, or use the host network.
# pg-wrap.sh is the reviewed entrypoint. up.sh is hash-pinned and not executed.
set -euo pipefail

PROOF=$(cd "$(dirname "$0")" && pwd)
EVIDENCE=$(cd "$PROOF/../.." && pwd)
REPO=$(cd "$EVIDENCE/../../../.." && pwd)
SEC=/tmp/oo-a380-eef831aa-dba4-4af2-a78b-541bc9e0a380
NET=oo-a380-net
PG_NAME=oo-a380-pg
PG_VOL=oo-a380-pgdata
PG_HOST=oo-a380-pg
BUILDER=oo-a380-proof
CARGO_VOL=oo-a380-cargo
TARGET_VOL=oo-a380-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a380
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
UP_HASH=904aed7ed10156576252da4be7ad828832aefe4628d15f67c1436f475cad9bff
WRAP_HASH=05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a
EXTRACT_HASH=3a630d6f8ef3ccb29412114a568cf17879054ac7a8fd9551af46ecf69be3c4a5
SHA_HASH=55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e
SELECT_HASH=54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644

mkdir -p "$EVIDENCE/raw" "$EVIDENCE/pg" "$EVIDENCE/linux/proof/phases"
git -C "$REPO" rev-parse HEAD > "$EVIDENCE/raw/head.txt"
git -C "$REPO" rev-parse '1078d69e8^{commit}' > "$EVIDENCE/raw/product-fixed.txt"
git -C "$REPO" diff --stat 1078d69e8 -- tools/gateway-audit-wal tools/audit-ingest >> "$EVIDENCE/raw/product-fixed.txt"
shasum -a 256 \
  "$REPO/deploy/trusted-demo/pg/up.sh" \
  "$REPO/deploy/trusted-demo/pg/pg-wrap.sh" \
  "$REPO/deploy/trusted-demo/linux/extract-libclang.sh" \
  "$REPO/deploy/trusted-demo/tools/sha256.rs" \
  "$REPO/deploy/trusted-demo/tools/select-debs.rs" \
  > "$EVIDENCE/raw/recipe-sha256.txt"
grep -q "$UP_HASH" "$EVIDENCE/raw/recipe-sha256.txt"
grep -q "$WRAP_HASH" "$EVIDENCE/raw/recipe-sha256.txt"
grep -q "$EXTRACT_HASH" "$EVIDENCE/raw/recipe-sha256.txt"
grep -q "$SHA_HASH" "$EVIDENCE/raw/recipe-sha256.txt"
grep -q "$SELECT_HASH" "$EVIDENCE/raw/recipe-sha256.txt"
test -f "$REPO/Cargo.toml"
test -f "$EVIDENCE/pg/query.sql"
test -f "$EVIDENCE/linux/proof/boundary.rs"

if ! docker image inspect "$RUST_IMAGE" >/dev/null 2>&1; then
  echo "pinned rust image is absent; not pulling" >&2
  exit 1
fi
if ! docker image inspect "$PG_IMAGE" >/dev/null 2>&1; then
  echo "pinned postgres image is absent; not pulling" >&2
  exit 1
fi
docker image inspect "$RUST_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/linux/proof/rust-image.txt"
docker image inspect "$PG_IMAGE" --format 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' > "$EVIDENCE/pg/image.txt"
grep -q 'arch=arm64' "$EVIDENCE/linux/proof/rust-image.txt"
grep -q 'arch=arm64' "$EVIDENCE/pg/image.txt"

{
  echo "builders"
  docker ps --filter label=oo.kind=rust-builder --format '{{.Names}} {{.Status}}'
  echo "postgres"
  docker ps --format '{{.Names}} {{.Image}} {{.Status}}'
  echo "host-tools"
  ps aux | awk '/[c]argo|[r]ustc/ { print }' || true
} > "$EVIDENCE/raw/capacity-before.txt"
if pgrep -x cargo >/dev/null 2>&1 || pgrep -x rustc >/dev/null 2>&1; then
  echo "host rust builder is already running" >&2
  exit 2
fi
builders=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | wc -l | tr -d ' ')
if [ "$builders" -ge 10 ]; then
  echo "rust builder cap is full" >&2
  exit 2
fi
if docker ps --format '{{.Names}}' | grep -qx "$BUILDER"; then
  echo "owned builder is already running" >&2
  exit 2
fi
if docker ps --format '{{.Names}}' | grep -qx "$PG_NAME"; then
  echo "owned postgres is already running" >&2
  exit 2
fi

mint_secrets() {
  umask 077
  mkdir -p "$SEC/pg" "$SEC/app/mtls"
  chmod 700 "$SEC" "$SEC/pg" "$SEC/app" "$SEC/app/mtls"
  openssl rand -hex 24 | tr -d '\n' > "$SEC/pg/admin.pass"
  openssl rand -hex 24 | tr -d '\n' > "$SEC/app/login.pass"
  openssl rand -hex 24 | tr -d '\n' > "$SEC/app/ingest.pass"
  cp "$SEC/pg/admin.pass" "$SEC/app/admin.pass"
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/pg/ca.key" -out "$SEC/pg/ca.crt" -days 7 -subj "/CN=oo-a380-pg-ca" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/pg/server.key" -out "$SEC/pg/server.csr" -subj "/CN=oo-a380-pg" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=DNS:oo-a380-pg\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' > "$SEC/pg/san.cnf"
  openssl x509 -req -in "$SEC/pg/server.csr" -CA "$SEC/pg/ca.crt" -CAkey "$SEC/pg/ca.key" -CAcreateserial -out "$SEC/pg/server.crt" -days 7 -extfile "$SEC/pg/san.cnf" >/dev/null 2>&1
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/app/mtls/ca.key" -out "$SEC/app/mtls/ca.crt" -days 7 -subj "/CN=oo-a380-audit-ca" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/app/mtls/server.key" -out "$SEC/app/mtls/server.csr" -subj "/CN=audit.local" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=DNS:audit.local\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' > "$SEC/app/mtls/server.cnf"
  openssl x509 -req -in "$SEC/app/mtls/server.csr" -CA "$SEC/app/mtls/ca.crt" -CAkey "$SEC/app/mtls/ca.key" -CAcreateserial -out "$SEC/app/mtls/server.crt" -days 7 -extfile "$SEC/app/mtls/server.cnf" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/app/mtls/client.key" -out "$SEC/app/mtls/client.csr" -subj "/CN=gateway-audit-wal" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=URI:spiffe://oo/workload/gateway-audit-wal\nextendedKeyUsage=clientAuth\nkeyUsage=digitalSignature\n' > "$SEC/app/mtls/client.cnf"
  openssl x509 -req -in "$SEC/app/mtls/client.csr" -CA "$SEC/app/mtls/ca.crt" -CAkey "$SEC/app/mtls/ca.key" -CAcreateserial -out "$SEC/app/mtls/client.crt" -days 7 -extfile "$SEC/app/mtls/client.cnf" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/app/mtls/stranger.key" -out "$SEC/app/mtls/stranger.csr" -subj "/CN=stranger" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=URI:spiffe://oo/workload/stranger\nextendedKeyUsage=clientAuth\nkeyUsage=digitalSignature\n' > "$SEC/app/mtls/stranger.cnf"
  openssl x509 -req -in "$SEC/app/mtls/stranger.csr" -CA "$SEC/app/mtls/ca.crt" -CAkey "$SEC/app/mtls/ca.key" -CAcreateserial -out "$SEC/app/mtls/stranger.crt" -days 7 -extfile "$SEC/app/mtls/stranger.cnf" >/dev/null 2>&1
  printf 'spiffe://oo/workload/gateway-audit-wal shop\n' > "$SEC/app/mtls/registry.txt"
  cp "$SEC/pg/ca.crt" "$SEC/app/pg-ca.crt"
  rm -f "$SEC/pg/ca.key" "$SEC/pg/server.csr" "$SEC/pg/san.cnf" "$SEC/pg/ca.srl" \
    "$SEC/app/mtls/ca.key" "$SEC/app/mtls/server.csr" "$SEC/app/mtls/client.csr" "$SEC/app/mtls/stranger.csr" \
    "$SEC/app/mtls/server.cnf" "$SEC/app/mtls/client.cnf" "$SEC/app/mtls/stranger.cnf" "$SEC/app/mtls/ca.srl"
  chmod 600 "$SEC/pg/admin.pass" "$SEC/pg/server.key" "$SEC/app/admin.pass" "$SEC/app/login.pass" "$SEC/app/ingest.pass" \
    "$SEC/app/mtls/server.key" "$SEC/app/mtls/client.key" "$SEC/app/mtls/stranger.key"
  chmod 644 "$SEC/pg/ca.crt" "$SEC/pg/server.crt" "$SEC/app/pg-ca.crt" "$SEC/app/mtls/ca.crt" \
    "$SEC/app/mtls/server.crt" "$SEC/app/mtls/client.crt" "$SEC/app/mtls/stranger.crt" "$SEC/app/mtls/registry.txt"
}

if [ ! -s "$SEC/app/login.pass" ] || [ ! -s "$SEC/app/mtls/client.key" ]; then
  rm -rf "$SEC"
  mint_secrets
fi

openssl x509 -in "$SEC/pg/server.crt" -noout -ext subjectAltName > "$EVIDENCE/pg/pg-san.txt"
openssl x509 -in "$SEC/app/mtls/server.crt" -noout -ext subjectAltName > "$EVIDENCE/linux/proof/audit-san.txt"
openssl x509 -in "$SEC/app/mtls/client.crt" -noout -ext subjectAltName > "$EVIDENCE/linux/proof/client-san.txt"
openssl x509 -in "$SEC/app/mtls/stranger.crt" -noout -ext subjectAltName > "$EVIDENCE/linux/proof/stranger-san.txt"
grep -q 'DNS:oo-a380-pg' "$EVIDENCE/pg/pg-san.txt"
grep -q 'DNS:audit.local' "$EVIDENCE/linux/proof/audit-san.txt"
grep -q 'URI:spiffe://oo/workload/gateway-audit-wal' "$EVIDENCE/linux/proof/client-san.txt"
grep -q 'URI:spiffe://oo/workload/stranger' "$EVIDENCE/linux/proof/stranger-san.txt"

login=$(cat "$SEC/app/login.pass")
ingest=$(cat "$SEC/app/ingest.pass")
if [ "${#login}" -ne 48 ] || [ "${#ingest}" -ne 48 ]; then
  echo "password shape" >&2
  exit 1
fi
case "$login$ingest" in
  *[!0-9a-f]*) echo "password shape" >&2; exit 1 ;;
esac
cat > "$SEC/roles.sql" << EOF
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_r_clerk') THEN
    CREATE ROLE oo_r_clerk NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'oo_a380_login') THEN
    CREATE ROLE oo_a380_login LOGIN PASSWORD '${login}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
  ELSE
    ALTER ROLE oo_a380_login PASSWORD '${login}';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audit_ingest') THEN
    CREATE ROLE audit_ingest LOGIN PASSWORD '${ingest}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
  ELSE
    ALTER ROLE audit_ingest PASSWORD '${ingest}';
  END IF;
END
\$\$;
GRANT oo_r_clerk TO oo_a380_login;
SELECT format('CREATE DATABASE %I TEMPLATE template0 ENCODING ''UTF8'' LOCALE_PROVIDER builtin BUILTIN_LOCALE ''C.UTF-8''', 'oo_query')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = 'oo_query')\\gexec
SELECT format('CREATE DATABASE %I TEMPLATE template0 ENCODING ''UTF8'' LOCALE_PROVIDER builtin BUILTIN_LOCALE ''C.UTF-8''', 'oo_audit')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = 'oo_audit')\\gexec
REVOKE CONNECT ON DATABASE oo_query FROM PUBLIC;
REVOKE CONNECT ON DATABASE oo_audit FROM PUBLIC;
GRANT CONNECT ON DATABASE oo_query TO oo_a380_login;
GRANT CONNECT ON DATABASE oo_audit TO audit_ingest;
EOF
chmod 600 "$SEC/roles.sql"
unset login ingest

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.owner=a380 \
    --label oo.session="$SESSION" \
    --label oo.kind=network \
    "$NET" >/dev/null
fi
for volume in "$PG_VOL" "$CARGO_VOL" "$TARGET_VOL"; do
  if ! docker volume inspect "$volume" >/dev/null 2>&1; then
    docker volume create --label oo.owner=a380 --label oo.session="$SESSION" "$volume" >/dev/null
  fi
done

redact() {
  local src="$1" dest="$2"
  awk -v a="$SEC/pg/admin.pass" -v b="$SEC/app/login.pass" -v c="$SEC/app/ingest.pass" '
    function load(path) {
      while ((getline line < path) > 0) {
        gsub(/\r/, "", line)
        if (line != "") n[++i] = line
      }
      close(path)
    }
    BEGIN { load(a); load(b); load(c) }
    {
      for (k = 1; k <= i; k++) if (index($0, n[k])) gsub(n[k], "[redacted]")
      print
    }
  ' "$src" > "$dest"
}

finish() {
  local status=$?
  docker logs "$PG_NAME" > "$SEC/postgres.log" 2>&1 || true
  docker logs "$BUILDER" > "$EVIDENCE/raw/builder.log" 2>&1 || true
  if [ -f "$SEC/postgres.log" ]; then
    redact "$SEC/postgres.log" "$EVIDENCE/pg/postgres.log"
  fi
  if [ -f "$SEC/roles.out" ]; then
    redact "$SEC/roles.out" "$EVIDENCE/pg/roles.out.redacted"
  fi
  if [ -f "$SEC/roles.err" ]; then
    redact "$SEC/roles.err" "$EVIDENCE/pg/roles.err.redacted"
  fi
  docker rm -f "$BUILDER" "$PG_NAME" >/dev/null 2>&1 || true
  if [ "$status" -eq 0 ]; then
    docker volume rm "$CARGO_VOL" "$TARGET_VOL" "$PG_VOL" >/dev/null 2>&1 || true
    docker network rm "$NET" >/dev/null 2>&1 || true
    rm -rf "$SEC"
    printf '%s\n' "removed oo-a380-proof oo-a380-pg oo-a380-cargo oo-a380-target oo-a380-pgdata oo-a380-net and the private secret directory" > "$EVIDENCE/CLEANUP.md"
    printf '%s\n' "none" > "$EVIDENCE/raw/failed-attempts.txt"
  else
    printf '%s\n' "removed oo-a380 containers; kept volumes, network, and the private secret directory" > "$EVIDENCE/CLEANUP.md"
    printf '%s\n' "launch status ${status}" > "$EVIDENCE/raw/failed-attempts.txt"
  fi
  exit "$status"
}
trap finish EXIT

docker run -d --name "$PG_NAME" \
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
  --network-alias "$PG_HOST" \
  -e POSTGRES_PASSWORD_FILE=/secrets/admin.pass \
  -e POSTGRES_USER=oo_fixture_admin \
  -e POSTGRES_DB=postgres \
  -e POSTGRES_INITDB_ARGS=--data-checksums \
  -e POSTGRES_HOST_AUTH_METHOD=scram-sha-256 \
  --mount "type=bind,source=${SEC}/pg,target=/secrets,readonly" \
  --mount "type=bind,source=${SEC}/pg,target=/tls-in,readonly" \
  --mount "type=volume,source=${PG_VOL},target=/var/lib/postgresql/data" \
  --mount "type=bind,source=${REPO}/deploy/trusted-demo/pg,target=/fixture,readonly" \
  --label oo.owner=a380 \
  --label oo.session="$SESSION" \
  --label oo.kind=postgres \
  --entrypoint bash \
  "$PG_IMAGE" /fixture/pg-wrap.sh > "$EVIDENCE/pg/container.id"

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if docker exec "$PG_NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  exit 1
fi
docker inspect "$PG_NAME" --format 'id={{.Id}} status={{.State.Status}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} cpus={{.HostConfig.NanoCpus}} pids={{.HostConfig.PidsLimit}}' > "$EVIDENCE/pg/inspect.txt"
docker inspect "$PG_NAME" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/pg/ports.json"
docker inspect "$PG_NAME" --format '{{json .HostConfig.CapDrop}} {{json .HostConfig.CapAdd}}' > "$EVIDENCE/pg/caps.json"

docker exec -i "$PG_NAME" psql -U oo_fixture_admin -d postgres -v ON_ERROR_STOP=1 -X -q \
  < "$SEC/roles.sql" > "$SEC/roles.out" 2> "$SEC/roles.err"
docker exec -i "$PG_NAME" psql -U oo_fixture_admin -d oo_query -v ON_ERROR_STOP=1 -X -q \
  < "$EVIDENCE/pg/query.sql" > "$EVIDENCE/pg/query.out" 2> "$EVIDENCE/pg/query.err"
docker exec -i "$PG_NAME" psql -U oo_fixture_admin -d oo_audit -v ON_ERROR_STOP=1 -X -q \
  < "$REPO/tools/audit-ingest/migrations/oo_audit_ingest_v1.sql" > "$EVIDENCE/pg/migration.out" 2> "$EVIDENCE/pg/migration.err"
docker exec "$PG_NAME" psql -U oo_fixture_admin -d postgres -Atqc "SELECT current_setting('server_version')" > "$EVIDENCE/pg/server_version.txt"
docker exec "$PG_NAME" psql -U oo_fixture_admin -d oo_query -Atqc "SELECT count(*) FROM _oo.pt_entry" > "$EVIDENCE/pg/rowcount.txt"
grep -q '^17\.10' "$EVIDENCE/pg/server_version.txt"
grep -qx '6' "$EVIDENCE/pg/rowcount.txt"

builders=$(docker ps --filter label=oo.kind=rust-builder --format '{{.Names}}' | wc -l | tr -d ' ')
if [ "$builders" -ge 10 ]; then
  echo "rust builder cap is full" >&2
  exit 2
fi

cat > "$EVIDENCE/linux/proof/invocation.txt" << EOF
builder=$BUILDER
image=$RUST_IMAGE
platform=linux/arm64
pull=never
memory=6g
cpus=1
pids=4096
network=$NET
tmpfs=/tmp:1g,/oo-wal-quota:262144
entrypoint=bash /evidence/linux/proof/inside.sh
host_network=no
privileged=no
docker_socket=no
repo=readonly
EOF

docker run -d --name "$BUILDER" \
  --platform linux/arm64 \
  --pull never \
  --init \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  --memory 6g \
  --memory-swap 6g \
  --cpus 1 \
  --pids-limit 4096 \
  --network "$NET" \
  --tmpfs /tmp:rw,nosuid,size=1073741824,mode=1777 \
  --tmpfs /oo-wal-quota:rw,nosuid,size=262144,mode=1777 \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_HOME=/usr/local/rustup \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e PATH=/usr/local/cargo/bin:/usr/bin:/bin \
  -e OO_A380_PG_HOST=oo-a380-pg \
  -e OO_A380_PG_PORT=5432 \
  -e OO_A380_QUERY_DB=oo_query \
  -e OO_A380_AUDIT_DB=oo_audit \
  -e OO_A380_LOGIN_USER=oo_a380_login \
  -e OO_A380_LOGIN_PASSWORD_FILE=/run/secrets/login.pass \
  -e OO_A380_ADMIN_USER=oo_fixture_admin \
  -e OO_A380_ADMIN_PASSWORD_FILE=/run/secrets/admin.pass \
  -e OO_A380_INGEST_USER=audit_ingest \
  -e OO_A380_INGEST_PASSWORD_FILE=/run/secrets/ingest.pass \
  -e OO_A380_PG_CA=/run/secrets/pg-ca.crt \
  -e OO_A380_MTLS=/run/secrets/mtls \
  -e OO_A380_QUOTA=/oo-wal-quota \
  -e OO_A380_RESULT=/evidence/raw/boundary-result.txt \
  -e OO_A380_REPO=/repo \
  -e OO_A380_BOUNDARY=1 \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo-home" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=bind,source=${REPO},target=/repo,readonly" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC}/app,target=/run/secrets,readonly" \
  --label oo.owner=a380 \
  --label oo.session="$SESSION" \
  --label oo.kind=rust-builder \
  --workdir /repo \
  --entrypoint bash \
  "$RUST_IMAGE" /evidence/linux/proof/inside.sh > "$EVIDENCE/linux/proof/container.id"

docker inspect "$BUILDER" --format 'id={{.Id}} status={{.State.Status}} privileged={{.HostConfig.Privileged}} network={{.HostConfig.NetworkMode}} memory={{.HostConfig.Memory}} cpus={{.HostConfig.NanoCpus}} pids={{.HostConfig.PidsLimit}}' > "$EVIDENCE/linux/proof/inspect.txt"
docker inspect "$BUILDER" --format '{{json .HostConfig.PortBindings}}' > "$EVIDENCE/linux/proof/ports.json"
docker inspect "$BUILDER" --format '{{json .HostConfig.Binds}} {{json .HostConfig.Mounts}}' > "$EVIDENCE/linux/proof/mounts.json"

status=$(docker wait "$BUILDER")
printf '%s\n' "$status" > "$EVIDENCE/raw/builder.exit"
if [ "$status" -ne 0 ]; then
  echo "builder exit ${status}" >&2
  exit "$status"
fi
grep -qx 'boundary=ok' "$EVIDENCE/raw/boundary-result.txt"
exit 0
