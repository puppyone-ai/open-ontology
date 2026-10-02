#!/bin/bash
# Admin seed. Passwords come from files through psql \getenv. Statement
# logging stays off until the roles and rows exist.
set -eu
umask 077
mkdir -p /pglog/admin

OO_READER_PW=$(tr -d '\n' < /secrets/reader.pass)
OO_GUC_PW=$(tr -d '\n' < /secrets/guc.pass)
OO_ALIAS_PW=$(tr -d '\n' < /secrets/alias.pass)
export OO_READER_PW OO_GUC_PW OO_ALIAS_PW

scrub() {
  file=$1
  [ -f "$file" ] || return 0
  data=$(cat "$file")
  data=${data//$OO_READER_PW/[redacted]}
  data=${data//$OO_GUC_PW/[redacted]}
  data=${data//$OO_ALIAS_PW/[redacted]}
  printf '%s\n' "$data" > "$file"
}

cat > /var/lib/postgresql/data/pg_hba.conf <<'EOF'
local all postgres trust
local all all trust
hostssl all all all scram-sha-256
hostnossl all all all reject
EOF
chown postgres:postgres /var/lib/postgresql/data/pg_hba.conf
chmod 600 /var/lib/postgresql/data/pg_hba.conf
gosu postgres psql -v ON_ERROR_STOP=1 -d postgres -c "SELECT pg_reload_conf()" > /pglog/admin/hba-reload.txt

set +e
gosu postgres psql -v ON_ERROR_STOP=1 -d postgres -f /sql/fixture/roles.sql > /pglog/admin/roles.out 2> /pglog/admin/roles.err
roles_ec=$?
set -e
scrub /pglog/admin/roles.out
scrub /pglog/admin/roles.err
printf '%s\n' "$roles_ec" > /pglog/admin/roles.exit
if [ "$roles_ec" -ne 0 ]; then
  echo "roles failed" >&2
  exit 1
fi

gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -f /sql/fixture/seed.sql > /pglog/admin/seed.out 2> /pglog/admin/seed.err

gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -Atc "SELECT n.nspname || '.' || p.proname FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE p.prosecdef AND n.nspname <> 'pg_catalog' AND n.nspname <> 'information_schema' AND n.nspname <> 'pg_toast' ORDER BY 1" > /pglog/admin/definers.txt

gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -At <<'SQL' > /pglog/admin/facts.json
SELECT json_build_object(
  'bpchar_hex', (SELECT encode(bpcharsend(padded), 'hex') FROM lab.gold WHERE id = 1),
  'jsonb_hex', (SELECT encode(jsonb_send(docb), 'hex') FROM lab.gold WHERE id = 1),
  'time24_hex', encode(time_send(TIME '24:00:00'), 'hex'),
  'units_hex', (SELECT encode(numeric_send(units), 'hex') FROM lab.gold WHERE id = 1),
  'place_hex', (SELECT encode(numeric_send(place), 'hex') FROM lab.gold WHERE id = 1),
  'negzero_hex', (SELECT encode(numeric_send(negzero), 'hex') FROM lab.gold WHERE id = 2),
  'flag_oid', (SELECT t.oid FROM pg_catalog.pg_type t JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace WHERE n.nspname = 'lab' AND t.typname = 'flag_domain'),
  'd_int_oid', (SELECT t.oid FROM pg_catalog.pg_type t JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace WHERE n.nspname = 'lab' AND t.typname = 'd_int'),
  'd_outer_oid', (SELECT t.oid FROM pg_catalog.pg_type t JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace WHERE n.nspname = 'lab' AND t.typname = 'd_outer'),
  'hue_oid', (SELECT t.oid FROM pg_catalog.pg_type t JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace WHERE n.nspname = 'lab' AND t.typname = 'hue'),
  'server_version', current_setting('server_version'),
  'server_version_num', current_setting('server_version_num')
)::text;
SQL

gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -At <<'SQL' > /pglog/admin/before-stats.txt
SELECT 'gold_md5', md5(string_agg(id::text || ':' || label || ':' || token, ',' ORDER BY id)) FROM lab.gold
UNION ALL
SELECT 'sink_count', count(*)::text FROM lab.canary_sink
UNION ALL
SELECT relname || ':ins', n_tup_ins::text FROM pg_stat_user_tables WHERE schemaname = 'lab' AND relname IN ('gold', 'canary_sink', 'anchor')
UNION ALL
SELECT relname || ':upd', n_tup_upd::text FROM pg_stat_user_tables WHERE schemaname = 'lab' AND relname IN ('gold', 'canary_sink', 'anchor')
UNION ALL
SELECT relname || ':del', n_tup_del::text FROM pg_stat_user_tables WHERE schemaname = 'lab' AND relname IN ('gold', 'canary_sink', 'anchor');
SQL

printf '%s:%s:%s:%s:%s\n' "127.0.0.1" "5432" "typedb" "oo_a110_reader" "$OO_READER_PW" > /tmp/a110.pgpass
printf '%s:%s:%s:%s:%s\n' "pg.a110.internal" "5432" "typedb" "oo_a110_reader" "$OO_READER_PW" >> /tmp/a110.pgpass
chmod 600 /tmp/a110.pgpass
set +e
PGPASSFILE=/tmp/a110.pgpass PGCONNECT_TIMEOUT=5 gosu postgres psql "host=127.0.0.1 port=5432 dbname=typedb user=oo_a110_reader sslmode=disable connect_timeout=5" -c "SELECT 1" > /pglog/admin/nonsql.out 2>&1
printf '%s\n' "$?" > /pglog/admin/nonsql.exit
PGPASSFILE=/tmp/a110.pgpass PGCONNECT_TIMEOUT=10 gosu postgres psql "host=pg.a110.internal hostaddr=127.0.0.1 port=5432 dbname=typedb user=oo_a110_reader sslmode=verify-full sslrootcert=/var/lib/postgresql/tls/ca.crt connect_timeout=10" -c "SELECT 1" > /pglog/admin/ssl.out 2>&1
printf '%s\n' "$?" > /pglog/admin/ssl.exit
set -e
rm -f /tmp/a110.pgpass
scrub /pglog/admin/nonsql.out
scrub /pglog/admin/ssl.out

gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -c "ALTER SYSTEM SET log_statement = 'all'"
gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -c "ALTER SYSTEM SET log_min_duration_statement = 0"
gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -c "SELECT pg_reload_conf()" > /pglog/admin/log-reload.txt
gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -Atc "SHOW log_statement" > /pglog/admin/show-log-statement.txt
gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -Atc "SHOW log_min_duration_statement" > /pglog/admin/show-log-duration.txt
gosu postgres psql -v ON_ERROR_STOP=1 -d typedb -Atc "SELECT 'A110-LOG-READY'" > /pglog/admin/marker-select.txt

found=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if [ -f /pglog/postgresql.log ] && grep -q 'A110-LOG-READY' /pglog/postgresql.log; then
    found=1
    break
  fi
  sleep 1
done
printf '%s\n' "$found" > /pglog/admin/marker.status
if [ -f /pglog/postgresql.log ]; then
  wc -c /pglog/postgresql.log > /pglog/admin/log-bytes-at-marker.txt
  grep -c 'A110-LOG-READY' /pglog/postgresql.log > /pglog/admin/marker.count || true
fi
if [ "$found" -ne 1 ]; then
  echo "statement marker was not logged" >&2
  exit 1
fi

unset OO_READER_PW OO_GUC_PW OO_ALIAS_PW
echo "seed_ok"
