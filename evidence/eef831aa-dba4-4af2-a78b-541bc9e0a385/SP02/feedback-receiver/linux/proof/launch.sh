#!/bin/bash
# One private PostgreSQL 17.10 and one Rust 1.94 builder.
# Does not pull, prune, or use the host network.
set -euo pipefail

PROOF=$(cd "$(dirname "$0")" && pwd)
EVIDENCE=$(cd "$PROOF/../.." && pwd)
REPO=$(cd "$EVIDENCE/../../../.." && pwd)
SEC=/tmp/oo-a385-eef831aa-dba4-4af2-a78b-541bc9e0a385
NET=oo-a385-net
PG_NAME=oo-a385-pg
PG_VOL=oo-a385-pgdata
PG_HOST=oo-a385-pg
BUILDER=oo-a385-proof
CARGO_VOL=oo-a385-cargo
TARGET_VOL=oo-a385-target
SESSION=eef831aa-dba4-4af2-a78b-541bc9e0a385
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
UP_HASH=904aed7ed10156576252da4be7ad828832aefe4628d15f67c1436f475cad9bff
WRAP_HASH=05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a

mkdir -p "$EVIDENCE/raw" "$EVIDENCE/pg" "$EVIDENCE/linux/proof/phases"
git -C "$REPO" rev-parse HEAD > "$EVIDENCE/raw/head.txt"
sha256sum "$REPO/deploy/trusted-demo/pg/up.sh" "$REPO/deploy/trusted-demo/pg/pg-wrap.sh" > "$EVIDENCE/raw/recipe-sha256.txt"
grep -q "$UP_HASH" "$EVIDENCE/raw/recipe-sha256.txt"
grep -q "$WRAP_HASH" "$EVIDENCE/raw/recipe-sha256.txt"
test -f "$REPO/tools/feedback-receiver/Cargo.toml"
test -f "$REPO/tools/feedback-receiver/Cargo.lock"
test -f "$REPO/tools/audit-ingest/migrations/oo_audit_ingest_v1.sql"
test -f "$REPO/tools/feedback-receiver/migrations/oo_feedback_receiver_v1.sql"

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
  docker ps --filter label=oo.kind=postgres --format '{{.Names}} {{.Status}}'
} > "$EVIDENCE/raw/capacity-before.txt"

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
  openssl rand -hex 24 | tr -d '\n' > "$SEC/app/feedback.pass"
  openssl rand -hex 24 | tr -d '\n' > "$SEC/app/audit.pass"
  cp "$SEC/pg/admin.pass" "$SEC/app/admin.pass"
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/pg/ca.key" -out "$SEC/pg/ca.crt" -days 7 -subj "/CN=oo-a385-pg-ca" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/pg/server.key" -out "$SEC/pg/server.csr" -subj "/CN=oo-a385-pg" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=DNS:oo-a385-pg\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' > "$SEC/pg/san.cnf"
  openssl x509 -req -in "$SEC/pg/server.csr" -CA "$SEC/pg/ca.crt" -CAkey "$SEC/pg/ca.key" -CAcreateserial -out "$SEC/pg/server.crt" -days 7 -extfile "$SEC/pg/san.cnf" >/dev/null 2>&1
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/app/mtls/ca.key" -out "$SEC/app/mtls/ca.crt" -days 7 -subj "/CN=oo-a385-feedback-ca" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/app/mtls/server.key" -out "$SEC/app/mtls/server.csr" -subj "/CN=feedback.local" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=DNS:feedback.local\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' > "$SEC/app/mtls/server.cnf"
  openssl x509 -req -in "$SEC/app/mtls/server.csr" -CA "$SEC/app/mtls/ca.crt" -CAkey "$SEC/app/mtls/ca.key" -CAcreateserial -out "$SEC/app/mtls/server.crt" -days 7 -extfile "$SEC/app/mtls/server.cnf" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/app/mtls/client.key" -out "$SEC/app/mtls/client.csr" -subj "/CN=feedback-gateway" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=URI:spiffe://oo/workload/feedback-gateway\nextendedKeyUsage=clientAuth\nkeyUsage=digitalSignature\n' > "$SEC/app/mtls/client.cnf"
  openssl x509 -req -in "$SEC/app/mtls/client.csr" -CA "$SEC/app/mtls/ca.crt" -CAkey "$SEC/app/mtls/ca.key" -CAcreateserial -out "$SEC/app/mtls/client.crt" -days 7 -extfile "$SEC/app/mtls/client.cnf" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/app/mtls/other.key" -out "$SEC/app/mtls/other.csr" -subj "/CN=feedback-other" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=URI:spiffe://oo/workload/feedback-other\nextendedKeyUsage=clientAuth\nkeyUsage=digitalSignature\n' > "$SEC/app/mtls/other.cnf"
  openssl x509 -req -in "$SEC/app/mtls/other.csr" -CA "$SEC/app/mtls/ca.crt" -CAkey "$SEC/app/mtls/ca.key" -CAcreateserial -out "$SEC/app/mtls/other.crt" -days 7 -extfile "$SEC/app/mtls/other.cnf" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/app/mtls/stranger.key" -out "$SEC/app/mtls/stranger.csr" -subj "/CN=stranger" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nsubjectAltName=URI:spiffe://oo/workload/stranger\nextendedKeyUsage=clientAuth\nkeyUsage=digitalSignature\n' > "$SEC/app/mtls/stranger.cnf"
  openssl x509 -req -in "$SEC/app/mtls/stranger.csr" -CA "$SEC/app/mtls/ca.crt" -CAkey "$SEC/app/mtls/ca.key" -CAcreateserial -out "$SEC/app/mtls/stranger.crt" -days 7 -extfile "$SEC/app/mtls/stranger.cnf" >/dev/null 2>&1
  printf 'spiffe://oo/workload/feedback-gateway shop\nspiffe://oo/workload/feedback-other other\n' > "$SEC/app/mtls/registry.txt"
  cp "$SEC/pg/ca.crt" "$SEC/app/pg-ca.crt"
  rm -f "$SEC/pg/ca.key" "$SEC/pg/server.csr" "$SEC/pg/san.cnf" "$SEC/pg/ca.srl" \
    "$SEC/app/mtls/ca.key" "$SEC/app/mtls/server.csr" "$SEC/app/mtls/client.csr" \
    "$SEC/app/mtls/other.csr" "$SEC/app/mtls/stranger.csr" \
    "$SEC/app/mtls/server.cnf" "$SEC/app/mtls/client.cnf" "$SEC/app/mtls/other.cnf" \
    "$SEC/app/mtls/stranger.cnf" "$SEC/app/mtls/ca.srl"
  chmod 600 "$SEC/pg/admin.pass" "$SEC/pg/server.key" "$SEC/app/admin.pass" \
    "$SEC/app/feedback.pass" "$SEC/app/audit.pass" \
    "$SEC/app/mtls/server.key" "$SEC/app/mtls/client.key" "$SEC/app/mtls/other.key" "$SEC/app/mtls/stranger.key"
  chmod 644 "$SEC/pg/ca.crt" "$SEC/pg/server.crt" "$SEC/app/pg-ca.crt" "$SEC/app/mtls/ca.crt" \
    "$SEC/app/mtls/server.crt" "$SEC/app/mtls/client.crt" "$SEC/app/mtls/other.crt" \
    "$SEC/app/mtls/stranger.crt" "$SEC/app/mtls/registry.txt"
}

if [ ! -s "$SEC/app/feedback.pass" ] || [ ! -s "$SEC/app/mtls/client.key" ] || [ ! -s "$SEC/app/mtls/other.key" ]; then
  rm -rf "$SEC"
  mint_secrets
fi

openssl x509 -in "$SEC/pg/server.crt" -noout -ext subjectAltName > "$EVIDENCE/pg/pg-san.txt"
openssl x509 -in "$SEC/app/mtls/server.crt" -noout -ext subjectAltName > "$EVIDENCE/linux/proof/server-san.txt"
openssl x509 -in "$SEC/app/mtls/client.crt" -noout -ext subjectAltName > "$EVIDENCE/linux/proof/client-san.txt"
openssl x509 -in "$SEC/app/mtls/other.crt" -noout -ext subjectAltName > "$EVIDENCE/linux/proof/other-san.txt"
openssl x509 -in "$SEC/app/mtls/stranger.crt" -noout -ext subjectAltName > "$EVIDENCE/linux/proof/stranger-san.txt"
grep -q 'DNS:oo-a385-pg' "$EVIDENCE/pg/pg-san.txt"
grep -q 'DNS:feedback.local' "$EVIDENCE/linux/proof/server-san.txt"
grep -q 'URI:spiffe://oo/workload/feedback-gateway' "$EVIDENCE/linux/proof/client-san.txt"
grep -q 'URI:spiffe://oo/workload/feedback-other' "$EVIDENCE/linux/proof/other-san.txt"
grep -q 'URI:spiffe://oo/workload/stranger' "$EVIDENCE/linux/proof/stranger-san.txt"
grep -q 'feedback-gateway shop' "$SEC/app/mtls/registry.txt"
grep -q 'feedback-other other' "$SEC/app/mtls/registry.txt"
if grep -q 'stranger' "$SEC/app/mtls/registry.txt"; then
  echo "stranger must stay out of the registry" >&2
  exit 1
fi

python3 - "$SEC" << 'PY'
import pathlib, re, sys
sec = pathlib.Path(sys.argv[1])
feedback = (sec / "app/feedback.pass").read_text().strip()
audit = (sec / "app/audit.pass").read_text().strip()
if re.fullmatch(r"[0-9a-f]{48}", feedback) is None or re.fullmatch(r"[0-9a-f]{48}", audit) is None:
    raise SystemExit("password shape")
text = f"""
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'feedback_receiver') THEN
    CREATE ROLE feedback_receiver LOGIN PASSWORD '{feedback}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
  ELSE
    ALTER ROLE feedback_receiver PASSWORD '{feedback}';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'audit_ingest') THEN
    CREATE ROLE audit_ingest LOGIN PASSWORD '{audit}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION;
  ELSE
    ALTER ROLE audit_ingest PASSWORD '{audit}';
  END IF;
END
$$;
SELECT format('CREATE DATABASE %I TEMPLATE template0 ENCODING ''UTF8'' LOCALE_PROVIDER builtin BUILTIN_LOCALE ''C.UTF-8''', 'oo_feedback')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = 'oo_feedback')\\gexec
SELECT format('CREATE DATABASE %I TEMPLATE template0 ENCODING ''UTF8'' LOCALE_PROVIDER builtin BUILTIN_LOCALE ''C.UTF-8''', 'oo_audit')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = 'oo_audit')\\gexec
REVOKE CONNECT ON DATABASE oo_feedback FROM PUBLIC;
REVOKE CONNECT ON DATABASE oo_audit FROM PUBLIC;
GRANT CONNECT ON DATABASE oo_feedback TO feedback_receiver;
GRANT CONNECT ON DATABASE oo_audit TO audit_ingest;
"""
path = sec / "roles.sql"
path.write_text(text)
path.chmod(0o600)
PY

if ! docker network inspect "$NET" >/dev/null 2>&1; then
  docker network create \
    --label oo.owner=a385 \
    --label oo.session="$SESSION" \
    --label oo.kind=network \
    "$NET" >/dev/null
fi
for volume in "$PG_VOL" "$CARGO_VOL" "$TARGET_VOL"; do
  if ! docker volume inspect "$volume" >/dev/null 2>&1; then
    docker volume create --label oo.owner=a385 --label oo.session="$SESSION" "$volume" >/dev/null
  fi
done

finish() {
  local status=$?
  docker logs "$PG_NAME" > "$EVIDENCE/pg/postgres.log" 2>&1 || true
  docker logs "$BUILDER" > "$EVIDENCE/raw/builder.log" 2>&1 || true
  docker rm -f "$BUILDER" "$PG_NAME" >/dev/null 2>&1 || true
  if [ "$status" -eq 0 ]; then
    docker volume rm "$CARGO_VOL" "$TARGET_VOL" "$PG_VOL" >/dev/null 2>&1 || true
    docker network rm "$NET" >/dev/null 2>&1 || true
    rm -rf "$SEC"
    printf '%s\n' "removed oo-a385-proof oo-a385-pg oo-a385-cargo oo-a385-target oo-a385-pgdata oo-a385-net and the private secret directory" > "$EVIDENCE/CLEANUP.md"
  else
    printf '%s\n' "removed oo-a385 containers; kept volumes, network, and the private secret directory" > "$EVIDENCE/CLEANUP.md"
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
  --label oo.owner=a385 \
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
docker exec -i "$PG_NAME" psql -U oo_fixture_admin -d oo_feedback -v ON_ERROR_STOP=1 -X -q \
  < "$REPO/tools/feedback-receiver/migrations/oo_feedback_receiver_v1.sql" > "$EVIDENCE/pg/feedback-migration.out" 2> "$EVIDENCE/pg/feedback-migration.err"
docker exec -i "$PG_NAME" psql -U oo_fixture_admin -d oo_audit -v ON_ERROR_STOP=1 -X -q \
  < "$REPO/tools/audit-ingest/migrations/oo_audit_ingest_v1.sql" > "$EVIDENCE/pg/audit-migration.out" 2> "$EVIDENCE/pg/audit-migration.err"
docker exec "$PG_NAME" psql -U oo_fixture_admin -d postgres -Atqc "SELECT current_setting('server_version')" > "$EVIDENCE/pg/server_version.txt"
grep -q '^17\.10' "$EVIDENCE/pg/server_version.txt"
cat > "$EVIDENCE/pg/privileges.sql" << 'SQL'
SELECT rolname, rolsuper, rolcreatedb, rolcreaterole, rolinherit, rolcanlogin, rolbypassrls, rolreplication
FROM pg_roles
WHERE rolname IN ('feedback_receiver', 'audit_ingest', 'oo_r_reviewer')
ORDER BY rolname;
SELECT datname
FROM pg_database
WHERE datname IN ('oo_feedback', 'oo_audit')
ORDER BY datname;
SELECT grantee, table_schema, table_name, privilege_type
FROM information_schema.role_table_grants
WHERE grantee IN ('feedback_receiver', 'audit_ingest')
ORDER BY 1, 2, 3, 4;
SELECT has_database_privilege('feedback_receiver', 'oo_audit', 'CONNECT') AS feedback_on_audit,
       has_database_privilege('audit_ingest', 'oo_feedback', 'CONNECT') AS audit_on_feedback,
       has_table_privilege('feedback_receiver', 'oo_feedback_receiver.receipt', 'INSERT') AS can_insert,
       has_table_privilege('feedback_receiver', 'oo_feedback_receiver.receipt', 'SELECT') AS can_select,
       has_table_privilege('feedback_receiver', 'oo_feedback_receiver.receipt', 'UPDATE') AS can_update,
       has_table_privilege('feedback_receiver', 'oo_feedback_receiver.receipt', 'DELETE') AS can_delete,
       has_table_privilege('feedback_receiver', 'oo_feedback_receiver.receipt', 'TRUNCATE') AS can_truncate;
SQL
docker exec -i "$PG_NAME" psql -U oo_fixture_admin -d oo_feedback -v ON_ERROR_STOP=1 -X \
  < "$EVIDENCE/pg/privileges.sql" > "$EVIDENCE/pg/privileges.txt" 2> "$EVIDENCE/pg/privileges.err"
grep -q 'feedback_receiver' "$EVIDENCE/pg/privileges.txt"
if grep -q 'oo_r_reviewer' "$EVIDENCE/pg/privileges.txt"; then
  echo "oo_r_reviewer must not be a database role" >&2
  exit 1
fi

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
tmpfs=/tmp:1g
entrypoint=bash /evidence/linux/proof/inside.sh
host_network=no
privileged=no
docker_socket=no
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
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_BUILD_JOBS=1 \
  -e RUSTUP_HOME=/usr/local/rustup \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e PATH=/usr/local/cargo/bin:/usr/bin:/bin \
  -e OO_A385_PG_HOST=oo-a385-pg \
  -e OO_A385_PG_PORT=5432 \
  -e OO_A385_PG_DB=oo_feedback \
  -e OO_A385_PG_USER=feedback_receiver \
  -e OO_A385_PG_PASSWORD_FILE=/run/secrets/feedback.pass \
  -e OO_A385_PG_CA=/run/secrets/pg-ca.crt \
  -e OO_A385_AUDIT_DB=oo_audit \
  -e OO_A385_AUDIT_PASSWORD_FILE=/run/secrets/audit.pass \
  -e OO_A385_MTLS=/run/secrets/mtls \
  -e OO_A385_RESULT=/evidence/raw/live-result.txt \
  -e OO_A385_STDERR=/evidence/raw/server-stderr.txt \
  -e OO_A385_LIVE=1 \
  --mount "type=volume,source=${CARGO_VOL},target=/cargo" \
  --mount "type=volume,source=${TARGET_VOL},target=/target" \
  --mount "type=bind,source=${REPO},target=/repo" \
  --mount "type=bind,source=${EVIDENCE},target=/evidence" \
  --mount "type=bind,source=${SEC}/app,target=/run/secrets,readonly" \
  --label oo.owner=a385 \
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
docker exec "$PG_NAME" psql -U oo_fixture_admin -d oo_feedback -Atqc \
  "SELECT project || '=' || count(*) FROM oo_feedback_receiver.receipt GROUP BY project ORDER BY project" \
  > "$EVIDENCE/pg/receipt-counts.txt" || true
python3 - "$SEC" "$EVIDENCE" << 'PY'
import pathlib, sys
sec = pathlib.Path(sys.argv[1])
evidence = pathlib.Path(sys.argv[2])
needles = []
for name in ["pg/admin.pass", "app/feedback.pass", "app/audit.pass", "app/admin.pass"]:
    raw = (sec / name).read_bytes().strip()
    if raw:
        needles.append(raw)
changed = False
for path in evidence.rglob("*"):
    if not path.is_file():
        continue
    blob = path.read_bytes()
    new = blob
    for needle in needles:
        new = new.replace(needle, b"[redacted]")
    if new != blob:
        path.write_bytes(new)
        changed = True
if changed:
    raise SystemExit("evidence contained a password and was redacted")
PY
if [ "$status" -ne 0 ]; then
  echo "builder exit ${status}" >&2
  exit "$status"
fi
grep -qx 'live=ok' "$EVIDENCE/raw/live-result.txt"
grep -qx 'shop=1' "$EVIDENCE/pg/receipt-counts.txt"
grep -qx 'other=1' "$EVIDENCE/pg/receipt-counts.txt"
exit 0
