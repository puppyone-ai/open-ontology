#!/bin/bash
# Fresh MariaDB 11.4.13 fixture for the driver preflight. Preparation SQL is read, not edited.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"

mkdir -p "$OO_EVIDENCE/raw" "$OO_EVIDENCE/inventory"
arch=$(docker image inspect "$OO_IMAGE_DIGEST" --format '{{.Architecture}}')
echo "image_arch=$arch"
if [ "$arch" != "arm64" ]; then
  echo "native arm64 image is not the inspected architecture" >&2
  exit 2
fi
docker tag "$OO_IMAGE_DIGEST" "$OO_IMAGE_TAG"
docker image inspect -f 'id={{.Id}} os={{.Os}} arch={{.Architecture}}' "$OO_IMAGE_DIGEST" > "$OO_EVIDENCE/raw/image.txt"

if lsof -nP -iTCP:"$OO_PORT" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "port 127.0.0.1:$OO_PORT is already in use" >&2
  exit 2
fi
if docker ps -a --format '{{.Names}}' | grep -qx "$OO_CONTAINER"; then
  echo "refusing existing container $OO_CONTAINER" >&2
  exit 2
fi

umask 077
if [ -e "$OO_SECRET_DIR" ]; then
  echo "refusing existing secret dir" >&2
  exit 2
fi
mkdir -p "$OO_SECRET_DIR"
chmod 700 "$OO_SECRET_DIR"
python3 - "$OO_SECRET_DIR" <<'PY'
import os
import pathlib
import secrets
import sys

directory = pathlib.Path(sys.argv[1])
os.umask(0o077)

def write_pw(name: str) -> None:
    path = directory / name
    path.write_text(secrets.token_hex(24), encoding="utf-8")
    path.chmod(0o600)

def write_cnf(user: str, pw_name: str, dest: str) -> None:
    password = (directory / pw_name).read_text(encoding="utf-8").strip()
    path = directory / dest
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
write_cnf("root", "root.pw", "root.cnf")
write_cnf("w39_dba", "dba.pw", "dba.cnf")
write_cnf("w39_reader", "reader.pw", "reader.cnf")
print("secrets=written")
PY

python3 "$OO_ROOT/scripts/admit_maria.py" "$OO_EVIDENCE/raw" -- bash "$OO_ROOT/scripts/docker-up.sh"

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

docker exec -i "$OO_CONTAINER" bash -s < "$OO_ROOT/scripts/bootstrap.sh"
oo_root < "$OO_ROOT/sql/04-grants.sql" >/dev/null
oo_dba < "$OO_PREP/sql/01-schema.sql" >/dev/null
oo_dba < "$OO_PREP/sql/02-data.sql" >/dev/null
oo_dba < "$OO_ROOT/sql/03-extensions.sql" >/dev/null

run_neg() {
  local name=$1
  local expect=$2
  local sql=$3
  local err=$OO_EVIDENCE/raw/${name}.stderr
  set +e
  printf '%s\n' "$sql" | oo_dba > "$OO_EVIDENCE/raw/${name}.stdout" 2> "$err"
  local status=$?
  set -e
  printf '%s\n' "$status" > "$OO_EVIDENCE/raw/${name}.exit"
  python3 "$OO_ROOT/scripts/secret_scan.py" "$OO_SECRET_DIR" "$err"
  if ! grep -q "ERROR $expect" "$err"; then
    echo "$name did not return ERROR $expect" >&2
    exit 1
  fi
  echo "${name}=ERROR ${expect}"
}

run_neg neg-zero-date 1292 "INSERT INTO oo_synth.k_zero_probe (id, d) VALUES (1, '0000-00-00');"
run_neg neg-unsigned 1264 "INSERT INTO oo_synth.k_unsigned (id, label) VALUES (-1, 'neg');"
run_neg neg-json 4025 "INSERT INTO oo_synth.k_json (id, doc) VALUES (99, 'not-json');"

oo_root <<'SQL' >/dev/null
SET GLOBAL general_log = OFF;
SET GLOBAL log_output = 'TABLE';
TRUNCATE TABLE mysql.general_log;
SET GLOBAL general_log = ON;
SQL
log_state=$(oo_root <<'SQL'
SELECT CONCAT(@@global.general_log + 0, '|', @@global.log_output);
SQL
)
echo "general_log=$log_state"
if [ "$log_state" != "1|TABLE" ]; then
  echo "general_log did not stay on TABLE" >&2
  exit 1
fi
identified=$(oo_root <<'SQL'
SELECT COUNT(*) FROM mysql.general_log WHERE argument LIKE CONCAT('%IDEN', 'TIFIED%');
SQL
)
echo "identified_by_rows=$identified"
if [ "$identified" != "0" ]; then
  echo "statement log contains account setup text" >&2
  exit 1
fi
echo "fixture=loaded"
