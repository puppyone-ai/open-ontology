#!/bin/bash
# Start one owned Rust 1.94 smoke container and one owned PostgreSQL fixture.
# Does not wait for the compiler. Does not touch the moving worktree.
set -eu
RUN=eef831aa-dba4-4af2-a78b-541bc9e0a097
ROOT=/Users/redux/puppy-one/open-ontology
BASE=/tmp/oo-$RUN
ARCH=$BASE/isolated-archive
PROBES=$BASE/probes
SEC=$BASE/secrets
EVID=$ROOT/evidence/$RUN/SP03/role-guc-review
PIN=34fae5ddd73ff250fde6f51aa681fb0f7fbd0362
RUST_IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
PG_IMAGE=sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb
NET=oo-a097-net-$RUN
RUST_NAME=oo-a097-msrv-$RUN
PG_NAME=oo-a097-pg-$RUN
CARGO_VOL=oo-a097-cargo-$RUN
TARGET_VOL=oo-a097-target-$RUN
PG_VOL=oo-a097-pglog-$RUN

mkdir -p "$ARCH" "$SEC" "$EVID/linux-smoke/phases" "$EVID/pg" "$EVID/commands"
chmod 700 "$SEC" "$BASE"
umask 077

echo "=== capacity before launch ==="
{
  echo "when=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host_rust"
  ps -ax -o pid=,ppid=,etime=,comm= | awk '$4 ~ /(^|\/)(cargo|rustc|rustdoc|clippy-driver)$/ {print}'
  echo "host_rust_count=$(ps -ax -o comm= | awk '$0 ~ /(^|\/)(cargo|rustc|rustdoc|clippy-driver)$/ {n++} END{print n+0}')"
  echo "docker_running"
  docker ps --filter status=running --format '{{.Names}} {{.Image}} {{.Status}}'
  echo "rust_running_count=$(docker ps --filter status=running --format '{{.Image}}' | awk '/0f24958fa693/ {n++} END{print n+0}')"
} | tee "$EVID/linux-smoke/capacity.txt"

rust_running=$(docker ps --filter status=running --format '{{.Image}}' | awk '/0f24958fa693/ {n++} END{print n+0}')
host_running=$(ps -ax -o comm= | awk '$0 ~ /(^|\/)(cargo|rustc|rustdoc|clippy-driver)$/ {n++} END{print n+0}')
if [ "$((rust_running + host_running))" -ge 2 ]; then
  echo "builder capacity full: rust=$rust_running host=$host_running" >&2
  exit 2
fi

if [ ! -f "$ARCH/Cargo.toml" ]; then
  git -C "$ROOT" archive "$PIN" | tar -x -C "$ARCH"
fi

{
  echo "run=$RUN"
  echo "commit=$(git -C "$ROOT" rev-parse "$PIN^{commit}")"
  echo "tree=$(git -C "$ROOT" rev-parse "$PIN^{tree}")"
  echo "parent=$(git -C "$ROOT" rev-parse "$PIN^")"
  echo "head_at_launch=$(git -C "$ROOT" rev-parse HEAD)"
  echo "archive_files=$(find "$ARCH" -type f | wc -l | tr -d ' ')"
  echo "git_files=$(git -C "$ROOT" ls-tree -r --name-only "$PIN" | wc -l | tr -d ' ')"
  echo "lock_sha256=$(shasum -a 256 "$ARCH/Cargo.lock" | awk '{print $1}')"
  echo "manifest_sha256=$(shasum -a 256 "$ARCH/crates/oo-connectors/Cargo.toml" | awk '{print $1}')"
  echo "docs17_sha256=$(shasum -a 256 "$ARCH/docs/17-rust-implementation.md" | awk '{print $1}')"
  echo "docs17_bytes=$(wc -c < "$ARCH/docs/17-rust-implementation.md" | tr -d ' ')"
  echo "docs17_lines=$(wc -l < "$ARCH/docs/17-rust-implementation.md" | tr -d ' ')"
  echo "parent_docs17_sha256=$(git -C "$ROOT" show "$PIN^:docs/17-rust-implementation.md" | shasum -a 256 | awk '{print $1}')"
} > "$EVID/linux-smoke/pin-draft.txt"

python3 - "$ARCH/specs/sources-baseline.json" "$EVID/linux-smoke/pin-draft.txt" <<'PY'
import json, sys
baseline = json.load(open(sys.argv[1]))
docs = next(item for item in baseline["sources"] if item["path"] == "docs/17-rust-implementation.md")
with open(sys.argv[2], "a", encoding="utf-8") as handle:
    handle.write(f"frozen_docs17_sha256={docs['sha256']}\n")
    handle.write(f"frozen_docs17_bytes={docs['bytes']}\n")
    handle.write(f"frozen_docs17_lines={docs['lines']}\n")
    handle.write(f"baseline_head={baseline['baseline_head']}\n")
PY

bash -n "$EVID/linux-smoke/inside.sh"
bash -n "$PROBES/load-fixture.sh"
shasum -a 256 "$EVID/linux-smoke/inside.sh" "$PROBES/load-fixture.sh" "$PROBES/extra.sql" > "$EVID/linux-smoke/input.sha256"

docker image inspect "$RUST_IMAGE" >/dev/null
docker image inspect "$PG_IMAGE" >/dev/null

openssl rand -hex 24 > "$SEC/super.pass"
openssl rand -hex 24 > "$SEC/reader.pass"
openssl rand -hex 24 > "$SEC/inherit.pass"
openssl rand -hex 24 > "$SEC/setonly.pass"
openssl rand -hex 24 > "$SEC/definer.pass"
for role in side hiddenfn schemacreate dbcreate dual transitive noset column bypass createrole createdb oponly castonly guc; do
  openssl rand -hex 24 > "$SEC/$role.pass"
done
openssl rand -hex 24 > "$SEC/inherit-only.pass"
openssl rand -hex 24 > "$SEC/login-super.pass"
for role in a097-plain a097-setnone a097-self a097-userguc a097-allow a097-dbuser a097-case a097-none a097-odd; do
  openssl rand -hex 24 > "$SEC/$role.pass"
done
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/ca.key" -out "$SEC/ca.crt" -days 30 -subj "/CN=oo-a097-ca" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$SEC/server.key" -out "$SEC/server.csr" -subj "/CN=oo-source-pg" >/dev/null 2>&1
cat > "$SEC/san.cnf" <<'EOF'
basicConstraints=CA:FALSE
subjectAltName=DNS:oo-source-pg
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
EOF
openssl x509 -req -in "$SEC/server.csr" -CA "$SEC/ca.crt" -CAkey "$SEC/ca.key" -CAcreateserial -out "$SEC/server.crt" -days 30 -extfile "$SEC/san.cnf" >/dev/null 2>&1
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$SEC/wrong-ca.key" -out "$SEC/wrong-ca.crt" -days 30 -subj "/CN=oo-a097-wrong-ca" >/dev/null 2>&1
chmod 600 "$SEC"/*
find "$SEC" -type f | wc -l | tr -d ' ' > "$EVID/pg/secret-file-count.txt"

docker network create \
  --label project=open-ontology --label owner=a097 --label scope=role-guc-review --label run="$RUN" \
  "$NET" >/dev/null
docker volume create --label project=open-ontology --label owner=a097 --label scope=role-guc-review --label run="$RUN" "$CARGO_VOL" >/dev/null
docker volume create --label project=open-ontology --label owner=a097 --label scope=role-guc-review --label run="$RUN" "$TARGET_VOL" >/dev/null
docker volume create --label project=open-ontology --label owner=a097 --label scope=role-guc-review --label run="$RUN" "$PG_VOL" >/dev/null

docker run -d --name "$PG_NAME" \
  --platform linux/arm64 \
  --pull never \
  --network "$NET" \
  --network-alias oo-source-pg \
  --network-alias oo-source-wrong \
  --memory 512m --memory-swap 512m --cpus 1 --pids-limit 256 \
  --label project=open-ontology --label owner=a097 --label scope=role-guc-review --label run="$RUN" \
  -e POSTGRES_PASSWORD_FILE=/secrets/super.pass \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_DB=postgres \
  -v "$SEC":/secrets:ro \
  -v "$SEC":/tls-in:ro \
  -v "$PG_VOL":/pglog \
  -v "$ARCH/testing/product/pg-source":/fixture:ro \
  -v "$PROBES":/a097:ro \
  --entrypoint /fixture/pg-wrap.sh \
  "$PG_IMAGE" >/dev/null

{
  echo "run=$RUN"
  echo "rust_image=$RUST_IMAGE"
  echo "pg_image=$PG_IMAGE"
  echo "platform=linux/arm64"
  echo "toolchain=1.94.0"
  echo "network=$NET"
  echo "rust_container=$RUST_NAME"
  echo "pg_container=$PG_NAME"
  echo "privileged=no"
  echo "network_mode=user_bridge_not_host"
  echo "published_ports=none"
  echo "home_mount=no"
  echo "socket_mount=no"
  echo "shared_cargo_cache=no"
  echo "pull=never"
  echo "cap_drop=ALL"
  echo "cap_add=none"
  echo "init=yes"
  echo "no_new_privileges=yes"
  echo "snapshot=$ARCH"
  echo "snapshot_method=git archive of $PIN only"
  echo "command=bash /inside.sh"
} > "$EVID/linux-smoke/invocation.txt"

docker run -d --name "$RUST_NAME" \
  --platform linux/arm64 \
  --pull never \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  --init \
  --memory 8g --memory-swap 8g --cpus 4 --pids-limit 4096 \
  --network "$NET" \
  --label project=open-ontology --label owner=a097 --label scope=role-guc-review --label run="$RUN" \
  -e RUSTUP_TOOLCHAIN=1.94.0 \
  -e HOME=/tmp \
  -e CARGO_HOME=/cargo \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_TERM_COLOR=never \
  -v "$ARCH":/src:ro \
  -v "$EVID/linux-smoke/phases":/phases \
  -v "$EVID/linux-smoke/inside.sh":/inside.sh:ro \
  -v "$CARGO_VOL":/cargo \
  -v "$TARGET_VOL":/target \
  -w /src \
  "$RUST_IMAGE" \
  bash /inside.sh >/dev/null

ready=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24; do
  if docker exec "$PG_NAME" pg_isready -U postgres >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 5
done
printf '%s\n' "$ready" > "$EVID/pg/ready.exit"
if [ "$ready" -ne 1 ]; then
  echo "postgres did not become ready" >&2
  exit 1
fi
docker exec "$PG_NAME" bash /a097/load-fixture.sh
docker exec "$PG_NAME" gosu postgres psql -Atc "SHOW server_version" > "$EVID/pg/server-version.txt"
docker exec "$PG_NAME" gosu postgres psql -Atc "SHOW server_version_num" > "$EVID/pg/server-version-num.txt"

python3 - "$SEC/pointer.json" <<'PY'
import json, sys
path = sys.argv[1]
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
with open(path, "w", encoding="utf-8") as handle:
    json.dump(body, handle)
    handle.write("\n")
PY
chmod 600 "$SEC/pointer.json"
cp "$SEC/pointer.json" "$EVID/pg/pointer.json"
echo "launch_ok rust=$RUST_NAME pg=$PG_NAME"
