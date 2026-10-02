#!/bin/bash
# One owned fixture and one owned Rust builder for the retention re-run.
set -eu
RUN=eef831aa-dba4-4af2-a78b-541bc9e0a097
ROOT=/Users/redux/puppy-one/open-ontology
BASE=/tmp/oo-$RUN
ARCH=$BASE/isolated-archive
SEC=$BASE/secrets
EVID=$ROOT/evidence/$RUN/SP03/role-guc-review/retention
PIN=34fae5ddd73ff250fde6f51aa681fb0f7fbd0362
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a097-ret-net-$RUN
PG_NAME=oo-a097-ret-pg-$RUN
RUST_NAME=oo-a097-ret-msrv-$RUN
CARGO_VOL=oo-a097-ret-cargo-$RUN
TARGET_VOL=oo-a097-ret-target-$RUN
PG_VOL=oo-a097-ret-pglog-$RUN

mkdir -p "$ARCH" "$SEC" "$EVID/rerun/phases"
chmod 700 "$SEC" "$BASE"
umask 077
cp "$EVID/rerun/a097_retention_rerun.rs" "$BASE/a097_retention_rerun.rs"

{
  echo "when=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /(^|\/)(cargo|rustc|rustdoc|clippy-driver)$/ {print}'
  docker ps --filter status=running --format '{{.Names}} {{.Image}} {{.Status}}'
} > "$EVID/rerun/capacity.txt"
rust_running=$(docker ps --filter status=running --format '{{.Image}}' | awk '/0f24958fa693/ {n++} END{print n+0}')
host_running=$(ps -ax -o comm= | awk '$0 ~ /(^|\/)(cargo|rustc|rustdoc|clippy-driver)$/ {n++} END{print n+0}')
echo "rust_running_count=$rust_running host_rust_count=$host_running" | tee -a "$EVID/rerun/capacity.txt"
if [ "$((rust_running + host_running))" -ge 2 ]; then
  echo "builder capacity full" >&2
  exit 2
fi

if [ ! -f "$ARCH/Cargo.toml" ]; then
  git -C "$ROOT" archive "$PIN" | tar -x -C "$ARCH"
fi
git -C "$ROOT" rev-parse "$PIN^{tree}" > "$EVID/rerun/archive-tree.txt"

for role in super reader inherit setonly definer side hiddenfn schemacreate dbcreate dual transitive noset column bypass createrole createdb oponly castonly guc; do
  if [ ! -f "$SEC/$role.pass" ]; then
    openssl rand -hex 24 > "$SEC/$role.pass"
  fi
done
if [ ! -f "$SEC/inherit-only.pass" ]; then openssl rand -hex 24 > "$SEC/inherit-only.pass"; fi
if [ ! -f "$SEC/login-super.pass" ]; then openssl rand -hex 24 > "$SEC/login-super.pass"; fi
if [ ! -f "$SEC/ca.crt" ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 30 -subj "/CN=oo-a097-ret-ca" >/dev/null 2>&1
  openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-source-pg" >/dev/null 2>&1
  cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-source-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
  openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 30 -extfile "$SEC/san.cnf" >/dev/null 2>&1
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/wrong-ca.key" -out "$SEC/wrong-ca.crt" -days 30 -subj "/CN=oo-a097-ret-wrong-ca" >/dev/null 2>&1
fi
chmod 600 "$SEC"/*

docker network create --label project=open-ontology --label owner=a097 --label scope=role-guc-retention --label run="$RUN" "$NET" >/dev/null
docker volume create --label project=open-ontology --label owner=a097 --label scope=role-guc-retention --label run="$RUN" "$PG_VOL" >/dev/null
docker volume create --label project=open-ontology --label owner=a097 --label scope=role-guc-retention --label run="$RUN" "$CARGO_VOL" >/dev/null
docker volume create --label project=open-ontology --label owner=a097 --label scope=role-guc-retention --label run="$RUN" "$TARGET_VOL" >/dev/null

docker run -d --name "$PG_NAME" \
  --platform linux/arm64 \
  --pull never \
  --network "$NET" \
  --network-alias oo-source-pg \
  --network-alias oo-source-wrong \
  --memory 512m --memory-swap 512m --cpus 1 --pids-limit 256 \
  --label project=open-ontology --label owner=a097 --label scope=role-guc-retention --label run="$RUN" \
  -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=postgres \
  -v "$SEC":/secrets:ro \
  -v "$SEC":/tls-in:ro \
  -v "$PG_VOL":/pglog \
  -v "$ARCH/testing/product/pg-source":/fixture:ro \
  --entrypoint /fixture/pg-wrap.sh \
  "$PG_IMAGE" >/dev/null

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24; do
  if docker exec "$PG_NAME" pg_isready -U postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 5
done
printf '%s\n' "$ready" > "$EVID/rerun/phases/pg-ready.exit"
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  exit 1
fi
docker exec "$PG_NAME" /fixture/load.sh > "$EVID/rerun/phases/load.txt" 2>&1

python3 - "$SEC/pointer.json" "$EVID/rerun/pointer.json" <<'PY'
import json, sys
body = {
    "host": "oo-source-pg",
    "port": 5432,
    "database": "source_lab",
    "empty_database": "source_empty",
    "reader_user": "oo_src_reader",
    "inherit_user": "oo_src_inherit",
    "setonly_user": "oo_src_setonly",
    "definer_user": "oo_src_definer",
    "side_user": "oo_src_side",
    "hiddenfn_user": "oo_src_hiddenfn",
    "schemacreate_user": "oo_src_schemacreate",
    "dbcreate_user": "oo_src_dbcreate",
    "dual_user": "oo_src_dual",
    "inherit_only_user": "oo_src_inherit_only",
    "transitive_user": "oo_src_transitive",
    "noset_user": "oo_src_noset",
    "column_user": "oo_src_column",
    "super_user": "oo_src_super",
    "bypass_user": "oo_src_bypass",
    "createrole_user": "oo_src_createrole",
    "createdb_user": "oo_src_createdb",
    "oponly_user": "oo_src_oponly",
    "castonly_user": "oo_src_castonly",
    "guc_user": "oo_src_guc_login",
    "password_dir": "/secrets",
    "ca_file": "/secrets/ca.crt",
    "wrong_ca_file": "/secrets/wrong-ca.crt",
    "wrong_host": "oo-source-wrong",
    "log_file": "/pglog/postgresql.log",
}
text = json.dumps(body)
for path in sys.argv[1:]:
    open(path, "w", encoding="utf-8").write(text)
PY
chmod 600 "$SEC/pointer.json"

docker run -d --name "$RUST_NAME" \
  --platform linux/arm64 \
  --pull never \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --init \
  --memory 8g --memory-swap 8g --cpus 4 --pids-limit 4096 \
  --network "$NET" \
  --label project=open-ontology --label owner=a097 --label scope=role-guc-retention --label run="$RUN" \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_TERM_COLOR=never \
  -e OO_PG_FIXTURE=/secrets/pointer.json \
  -v "$ARCH":/src:ro \
  -v "$SEC":/secrets:ro \
  -v "$PG_VOL":/pglog:ro \
  -v "$EVID/rerun/phases":/phases \
  -v "$BASE/a097_retention_rerun.rs":/opt/a097/a097_retention_rerun.rs:ro \
  -v "$EVID/rerun/inside.sh":/inside.sh:ro \
  -v "$CARGO_VOL":/cargo \
  -v "$TARGET_VOL":/target \
  -w /src \
  "$RUST_IMAGE" \
  bash /inside.sh >/dev/null

echo "launch_ok name=$RUST_NAME"
