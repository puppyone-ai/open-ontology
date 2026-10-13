#!/bin/bash
# Start a fresh MariaDB 11.4.13 fixture and load it as oo_dba.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"

mkdir -p "$OO_EVIDENCE/raw" "$OO_EVIDENCE/inventory" "$OO_ROOT/inventory"

arch=$(docker image inspect "$OO_IMAGE_DIGEST" --format '{{.Architecture}}')
echo "image_arch=$arch"
if [ "$arch" != "arm64" ]; then
  echo "native arm64 image is not the inspected architecture; refusing QEMU" >&2
  exit 2
fi
docker tag "$OO_IMAGE_DIGEST" "$OO_IMAGE_TAG"

if lsof -nP -iTCP:"$OO_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "port 127.0.0.1:$OO_PORT is already in use" >&2
  exit 2
fi

umask 077
mkdir -p "$OO_SECRET_DIR"
chmod 700 "$OO_SECRET_DIR"
python3 - "$OO_SECRET_DIR" <<'PY'
import os
import pathlib
import secrets
import sys

directory = pathlib.Path(sys.argv[1])
os.umask(0o077)

def write_pw(name: str) -> str:
    value = secrets.token_hex(24)
    path = directory / name
    path.write_text(value, encoding="utf-8")
    path.chmod(0o600)
    return value

def write_cnf(user: str, pw_name: str) -> None:
    password = (directory / pw_name).read_text(encoding="utf-8").strip()
    path = directory / f"{user}.cnf"
    path.write_text(
        "[client]\n"
        f"user={user}\n"
        f"password={password}\n"
        "protocol=socket\n"
        "socket=/run/mysqld/mysqld.sock\n",
        encoding="utf-8",
    )
    path.chmod(0o600)

write_pw("root.pw")
write_pw("dba.pw")
write_pw("reader.pw")
write_cnf("root", "root.pw")
write_cnf("oo_dba", "dba.pw")
write_cnf("oo_reader", "reader.pw")
# The server account is oo_dba. The defaults file name stays dba.cnf for the scripts.
(directory / "dba.cnf").write_text((directory / "oo_dba.cnf").read_text(encoding="utf-8"), encoding="utf-8")
(directory / "dba.cnf").chmod(0o600)
(directory / "reader.cnf").write_text((directory / "oo_reader.cnf").read_text(encoding="utf-8"), encoding="utf-8")
(directory / "reader.cnf").chmod(0o600)
print("secrets=written")
PY

python3 "$OO_ROOT/scripts/admit_start.py" bash "$OO_ROOT/scripts/docker-up.sh"

ready=0
for _ in $(seq 1 90); do
  if docker exec "$OO_CONTAINER" mariadb \
    --defaults-extra-file=/run/secrets/root.cnf \
    --protocol=socket \
    --socket=/run/mysqld/mysqld.sock \
    --silent --skip-column-names \
    -e "SELECT 1" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" -ne 1 ]; then
  echo "server did not accept a root socket connection" >&2
  exit 1
fi
echo "server_ready=yes"

docker exec "$OO_CONTAINER" bash /opt/oo-prep/bootstrap.sh
oo_root < "$OO_ROOT/sql/03-grants.sql" >/dev/null
oo_dba < "$OO_ROOT/sql/01-schema.sql" >/dev/null
oo_dba < "$OO_ROOT/sql/02-data.sql" >/dev/null

# Log tables reject TRUNCATE while general_log is ON. Clear first, then enable.
oo_root <<'SQL' >/dev/null
SET GLOBAL general_log = OFF;
SET GLOBAL log_output = 'TABLE';
TRUNCATE TABLE mysql.general_log;
SET GLOBAL general_log = ON;
SQL

# The client renders boolean system variables as ON/OFF. +0 keeps this check numeric.
log_state=$(oo_root <<'SQL'
SELECT CONCAT(@@global.general_log + 0, '|', @@global.log_output);
SQL
)
echo "general_log=$log_state"
if [ "$log_state" != "1|TABLE" ]; then
  echo "general_log did not stay on TABLE" >&2
  exit 1
fi

# Split the marker so this query is not logged as a match for itself.
identified=$(oo_root <<'SQL'
SELECT COUNT(*) FROM mysql.general_log WHERE argument LIKE CONCAT('%IDEN', 'TIFIED%');
SQL
)
echo "identified_by_rows=$identified"
if [ "$identified" != "0" ]; then
  echo "statement log contains account setup text" >&2
  exit 1
fi

log_tmp=$(mktemp)
docker logs "$OO_CONTAINER" > "$log_tmp" 2>&1 || true
if ! python3 "$OO_ROOT/scripts/secret_scan.py" "$OO_SECRET_DIR" "$log_tmp"; then
  rm -f "$log_tmp"
  echo "docker logs contained a secret and were not copied" >&2
  exit 1
fi
mv "$log_tmp" "$OO_EVIDENCE/raw/docker-logs.txt"

inspect_tmp=$(mktemp)
docker inspect "$OO_CONTAINER" > "$inspect_tmp"
python3 - "$OO_SECRET_DIR" "$inspect_tmp" "$OO_EVIDENCE/raw/inspect.redacted.json" <<'PY'
import json
import pathlib
import sys

secret_dir = pathlib.Path(sys.argv[1])
secrets = []
for path in secret_dir.glob("*.pw"):
    text = path.read_text(encoding="utf-8").strip()
    if text:
        secrets.append(text)
raw = pathlib.Path(sys.argv[2]).read_text(encoding="utf-8")
count = 0
redacted = raw
for secret in secrets:
    count += redacted.count(secret)
    redacted = redacted.replace(secret, "<redacted>")
pathlib.Path(sys.argv[3]).write_text(redacted, encoding="utf-8")
print(f"inspect_secret_redactions={count}")
if count:
    raise SystemExit(1)
PY
rm -f "$inspect_tmp"

python3 "$OO_ROOT/scripts/secret_scan.py" "$OO_SECRET_DIR" "$OO_EVIDENCE/raw/inspect.redacted.json"

{
  echo "container=$OO_CONTAINER"
  echo "network=$OO_NETWORK"
  echo "volume=$OO_VOLUME"
  echo "publish=127.0.0.1:${OO_PORT}"
  echo "memory=$OO_MEMORY"
  echo "image_tag=$OO_IMAGE_TAG"
  echo "image_digest=$OO_IMAGE_DIGEST"
  docker inspect -f 'image_id={{.Image}}' "$OO_CONTAINER"
  docker inspect -f 'platform={{.Platform}}' "$OO_CONTAINER" || true
  docker exec "$OO_CONTAINER" mariadb --version
  client_bin=$(docker exec "$OO_CONTAINER" bash -c 'command -v mariadb')
  server_bin=$(docker exec "$OO_CONTAINER" bash -c 'command -v mariadbd')
  docker exec "$OO_CONTAINER" sha256sum "$client_bin"
  docker exec "$OO_CONTAINER" sha256sum "$server_bin"
  docker exec "$OO_CONTAINER" dpkg-query -W -f 'package=${Package} version=${Version} arch=${Architecture}\n' 'mariadb*' \
    | grep -E 'version=[^[:space:]]'
} > "$OO_EVIDENCE/raw/runtime-identity.txt"

bash "$OO_ROOT/scripts/collect_inventory.sh"

echo "prepare=ok"
