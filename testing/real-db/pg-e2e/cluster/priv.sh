#!/bin/bash
# One writer rollback, one reader insert, one administrator cancel.
# Runs inside PostgreSQL. Does not print password bytes.
set -eu
umask 077
OUT=/tmp/a317-priv-out
SEC=/tmp/a317-priv-secret
mkdir -p "$OUT" "$SEC"
chmod 700 "$SEC"
reader=$(tr -d '\r\n' < /secrets/reader.pass)
writer=$(tr -d '\r\n' < /secrets/writer.pass)
admin=$(tr -d '\r\n' < /secrets/admin.pass)
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_a317_writer "$writer" > "$SEC/writer.pgpass"
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_a317_reader "$reader" > "$SEC/reader.pgpass"
printf '%s:%s:%s:%s:%s\n' '*' '*' '*' oo_fixture_admin "$admin" > "$SEC/admin.pgpass"
chmod 600 "$SEC"/*.pgpass

export PGPASSFILE=$SEC/writer.pgpass
psql -h 127.0.0.1 -U oo_a317_writer -d oo_a317_src -v ON_ERROR_STOP=1 -v VERBOSITY=verbose > "$OUT/writer.out" 2> "$OUT/writer.err" <<'SQL'
BEGIN;
INSERT INTO shop.party (party_key, display_name, active, phone, national_id) VALUES ('p90000', 'probe', true, 'tel-probe', 'nid-probe');
ROLLBACK;
SQL
printf '%s\n' 0 > "$OUT/writer.exit"

export PGPASSFILE=$SEC/reader.pgpass
set +e
psql -h 127.0.0.1 -U oo_a317_reader -d oo_a317_src -v ON_ERROR_STOP=1 -v VERBOSITY=verbose -c "INSERT INTO shop.party (party_key, display_name, active, phone, national_id) VALUES ('p90000', 'probe', true, 'tel-probe', 'nid-probe')" > "$OUT/reader.out" 2> "$OUT/reader.err"
printf '%s\n' "$?" > "$OUT/reader.exit"
set -e

export PGPASSFILE=$SEC/reader.pgpass
psql -h 127.0.0.1 -U oo_a317_reader -d oo_a317_src -v ON_ERROR_STOP=1 -v VERBOSITY=verbose -c "SELECT pg_sleep(60)" > "$OUT/sleep.out" 2> "$OUT/sleep.err" &
sleep_pid=$!
export PGPASSFILE=$SEC/admin.pgpass
pid=""
i=0
while [ "$i" -lt 30 ]; do
  pid=$(psql -h 127.0.0.1 -U oo_fixture_admin -d postgres -Atqc "SELECT pid FROM pg_stat_activity WHERE usename = 'oo_a317_reader' AND query LIKE 'SELECT pg_sleep(60)%' ORDER BY pid LIMIT 1" || true)
  case "$pid" in
    ''|*[!0-9]*) pid="" ;;
  esac
  if [ -n "$pid" ]; then
    break
  fi
  i=$((i + 1))
  sleep 1
done
if [ -z "$pid" ]; then
  echo "no sleep backend" > "$OUT/cancel.err"
  printf '%s\n' 1 > "$OUT/cancel.exit"
  kill "$sleep_pid" 2>/dev/null || true
  wait "$sleep_pid" 2>/dev/null || true
  exit 1
fi
psql -h 127.0.0.1 -U oo_fixture_admin -d postgres -v ON_ERROR_STOP=1 -c "SELECT pg_cancel_backend(${pid})" > "$OUT/cancel-admin.out"
set +e
wait "$sleep_pid"
printf '%s\n' "$?" > "$OUT/sleep.exit"
set -e
printf '%s\n' 0 > "$OUT/cancel.exit"
