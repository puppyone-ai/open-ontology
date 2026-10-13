#!/bin/bash
# Cancel one reader statement from the DBA identity. One business connection.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"
mkdir -p "$OO_EVIDENCE/raw"

out="$OO_EVIDENCE/raw/cancel-reader.stdout"
err="$OO_EVIDENCE/raw/cancel-reader.stderr"
: >"$out"
: >"$err"
start=$SECONDS
docker exec "$OO_CONTAINER" mariadb \
  --defaults-extra-file=/run/secrets/reader.cnf \
  --protocol=socket \
  --socket=/run/mysqld/mysqld.sock \
  --default-character-set=utf8mb4 \
  --comments \
  --batch --raw --silent --skip-column-names \
  oo_synth \
  -e "SELECT SLEEP(25) /*oo-dl01w1-08-cancel*/" \
  >"$out" 2>"$err" &
reader_pid=$!

query_id=""
for _ in $(seq 1 50); do
  query_id=$(printf '%s\n' "SELECT ID FROM information_schema.PROCESSLIST WHERE USER = 'oo_reader' AND INFO LIKE '%oo-dl01w1-08-cancel%' LIMIT 1;" | oo_dba || true)
  query_id=$(printf '%s' "$query_id" | tr -d '[:space:]')
  if printf '%s' "$query_id" | grep -Eq '^[0-9]+$'; then
    break
  fi
  query_id=""
  sleep 0.2
done
if [ -z "$query_id" ]; then
  kill "$reader_pid" >/dev/null 2>&1 || true
  wait "$reader_pid" || true
  echo "cancel target did not appear in PROCESSLIST" >&2
  exit 1
fi
echo "cancel_query_id=$query_id"
printf 'KILL QUERY %s;\n' "$query_id" | oo_dba >/dev/null

set +e
wait "$reader_pid"
reader_ec=$?
set -e
elapsed=$((SECONDS - start))
echo "reader_exit=$reader_ec"
echo "elapsed_seconds=$elapsed"
python3 "$OO_ROOT/scripts/secret_scan.py" "$OO_SECRET_DIR" "$out"
python3 "$OO_ROOT/scripts/secret_scan.py" "$OO_SECRET_DIR" "$err"
if [ "$elapsed" -ge 15 ]; then
  echo "cancel did not stop the reader statement in time" >&2
  exit 1
fi
interrupted=0
if grep -Eiq '1317|interrupted' "$err"; then
  interrupted=1
fi
if grep -Eq '^1$' "$out"; then
  interrupted=1
fi
if [ "$interrupted" -ne 1 ]; then
  echo "reader finished without an interrupt result" >&2
  exit 1
fi
if grep -Eq '^0$' "$out"; then
  echo "SLEEP completed" >&2
  exit 1
fi
echo "cancel=ok"
