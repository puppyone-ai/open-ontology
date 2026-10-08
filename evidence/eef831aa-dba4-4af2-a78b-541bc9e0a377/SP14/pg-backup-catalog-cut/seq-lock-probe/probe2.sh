#!/bin/bash
# Same-backend cache and non-superuser role. PostgreSQL 17.10 on the owned server.
set -u
OUT=/tmp/a377-seq-probe2
rm -rf "$OUT"
mkdir -p "$OUT"
chmod 700 "$OUT"
umask 077
pass=$(tr -d '\r\n' < /var/lib/postgresql/a377-admin.pass)
pf=$(mktemp /tmp/a377.pgpass.XXXXXX)
printf '%s\n' "/var/run/postgresql:5432:*:oo_a356_admin:${pass}" > "$pf"
chmod 600 "$pf"
export PGPASSFILE=$pf PGHOST=/var/run/postgresql PGUSER=oo_a356_admin PGPORT=5432
unset pass
note() { printf '%s\n' "$1" >> "$OUT/result.txt"; }

psql --no-password --no-psqlrc -v ON_ERROR_STOP=1 -d oo_a377_seqprobe -c "CREATE SEQUENCE shop.cache_hold AS bigint CACHE 10 START WITH 1;"

cat > "$OUT/cache.sql" <<'SQL'
\set VERBOSITY verbose
\set ON_ERROR_STOP off
SELECT 'first|' || nextval('shop.cache_hold')::text;
\! sleep 8
SET lock_timeout = '1s';
SELECT 'second|' || nextval('shop.cache_hold')::text;
SQL

psql --no-password --no-psqlrc -d oo_a377_seqprobe -f "$OUT/cache.sql" > "$OUT/cache.out" 2> "$OUT/cache.err" &
cache_pid=$!
sleep 1.5

cat > "$OUT/hold.sql" <<'SQL'
BEGIN ISOLATION LEVEL REPEATABLE READ;
ALTER SEQUENCE shop.cache_hold OWNED BY NONE;
SELECT 'during|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text || '|' || (SELECT relfilenode::text FROM pg_class WHERE oid = 'shop.cache_hold'::regclass) FROM shop.cache_hold;
SELECT pg_sleep(12);
COMMIT;
SQL
psql --no-password --no-psqlrc -d oo_a377_seqprobe -f "$OUT/hold.sql" > "$OUT/hold.out" 2> "$OUT/hold.err"
wait "$cache_pid" || true

note "cache_out=$(tr '\n' ';' < "$OUT/cache.out")"
note "cache_err=$(tr '\n' ';' < "$OUT/cache.err")"
note "hold_out=$(tr '\n' ';' < "$OUT/hold.out")"
note "hold_err=$(tr '\n' ';' < "$OUT/hold.err")"

psql --no-password --no-psqlrc -v ON_ERROR_STOP=1 -d oo_a377_seqprobe <<'SQL' > "$OUT/role.out" 2> "$OUT/role.err"
BEGIN;
GRANT oo_a356_admin TO oo_a377_operator;
GRANT oo_a377_operator TO oo_a356_admin;
SET ROLE oo_a377_operator;
SELECT 'who|' || current_user || '|' || session_user || '|' || CASE WHEN r.rolsuper THEN 't' ELSE 'f' END
FROM pg_roles r WHERE r.rolname = current_user;
LOCK TABLE shop.entry IN SHARE MODE;
ALTER SEQUENCE shop.cut_seq OWNED BY NONE;
SELECT 'state|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM shop.cut_seq;
SELECT 'mode|' || l.mode
FROM pg_locks l
JOIN pg_class c ON c.oid = l.relation
WHERE c.relname = 'cut_seq' AND l.pid = pg_backend_pid() AND l.granted
ORDER BY 1;
ROLLBACK;
SQL
note "role_out=$(tr '\n' ';' < "$OUT/role.out")"
note "role_err=$(tr '\n' ';' < "$OUT/role.err")"

psql --no-password --no-psqlrc -d oo_a377_seqprobe <<'SQL' > "$OUT/stranger.out" 2> "$OUT/stranger.err" || true
BEGIN;
GRANT USAGE ON SCHEMA shop TO oo_a377_stranger;
SET ROLE oo_a377_stranger;
LOCK TABLE shop.entry IN SHARE MODE;
ALTER SEQUENCE shop.cut_seq OWNED BY NONE;
ROLLBACK;
SQL
note "stranger_err=$(tr '\n' ';' < "$OUT/stranger.err")"
cp "$OUT/result.txt" /run-in/seq-probe2-result.txt
printf '%s\n' done > /run-in/seq-probe2.exit
