#!/bin/bash
# Runtime-role probes. The password stays in the container pgpass file and is removed here.
set -eu
umask 077
trap 'rm -f /tmp/a396.pgpass' EXIT
printf 'pg:5432:oo_a390_control:oo_a390_runtime_login:%s\n' "$(cat /secrets/runtime.pass)" > /tmp/a396.pgpass
chmod 600 /tmp/a396.pgpass
export PGPASSFILE=/tmp/a396.pgpass
export PGSSLMODE=verify-full
export PGSSLROOTCERT=/secrets/ca.crt
export PGHOST=pg
export PGPORT=5432
export PGDATABASE=oo_a390_control
export PGUSER=oo_a390_runtime_login
psql -v ON_ERROR_STOP=1 -X -f /oracle/catalog.sql
echo '=== function definitions as database owner ==='
# Local trust socket. Drop the runtime TCP environment so the admin role is not sent to host pg without a password.
admin_env='env -u PGHOST -u PGPORT -u PGUSER -u PGPASSFILE -u PGDATABASE -u PGSSLMODE -u PGSSLROOTCERT -u PGSERVICE'
if command -v runuser >/dev/null 2>&1; then
  runuser -u postgres -- $admin_env psql -h /var/run/postgresql -U oo_fixture_admin -d oo_a390_control -v ON_ERROR_STOP=1 -X -f /oracle/catalog-def.sql
else
  su -s /bin/bash postgres -c "$admin_env psql -h /var/run/postgresql -U oo_fixture_admin -d oo_a390_control -v ON_ERROR_STOP=1 -X -f /oracle/catalog-def.sql"
fi
echo '=== jitter ==='
psql -v ON_ERROR_STOP=1 -X -f /oracle/jitter.sql
echo '=== race ==='
psql -v ON_ERROR_STOP=1 -X -f /oracle/race-hold.sql > /tmp/race-a.out 2> /tmp/race-a.err &
holder=$!
sleep 0.4
set +e
psql -v ON_ERROR_STOP=1 -X -f /oracle/race-wait.sql > /tmp/race-b.out 2> /tmp/race-b.err
waiter=$?
set +e
wait "$holder"
holder_exit=$?
set -e
echo "holder_exit=$holder_exit"
echo "waiter_exit=$waiter"
echo '--- holder stdout ---'
cat /tmp/race-a.out
echo '--- holder stderr ---'
cat /tmp/race-a.err
echo '--- waiter stdout ---'
cat /tmp/race-b.out
echo '--- waiter stderr ---'
cat /tmp/race-b.err
echo '=== race count ==='
psql -v ON_ERROR_STOP=1 -X -c "SELECT count(*)::bigint AS race_rows, min(state) AS state FROM jobs.tasks WHERE dedupe_key = 'review-race';"
echo '=== enqueue review-once ==='
psql -v ON_ERROR_STOP=1 -X -f /oracle/enqueue-once.sql
echo '=== stats before serve ==='
psql -v ON_ERROR_STOP=1 -X -c "SELECT n.nspname || '.' || c.relname AS rel, s.n_tup_ins, s.n_tup_upd FROM pg_stat_user_tables s JOIN pg_class c ON c.oid = s.relid JOIN pg_namespace n ON n.oid = c.relnamespace ORDER BY 1;"
