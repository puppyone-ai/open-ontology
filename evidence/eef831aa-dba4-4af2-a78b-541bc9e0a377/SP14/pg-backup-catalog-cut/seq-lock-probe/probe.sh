#!/bin/bash
# Measure PostgreSQL 17.10 sequence exclusion. No sequence value is edited on purpose.
# Password is read inside the container. Results are sqlstate, lock mode, and sequence fields.
set -u
OUT=/tmp/a377-seq-probe
rm -rf "$OUT"
mkdir -p "$OUT"
chmod 700 "$OUT"
umask 077
pass=$(tr -d '\r\n' < /var/lib/postgresql/a377-admin.pass)
pf=$(mktemp /tmp/a377.pgpass.XXXXXX)
printf '%s\n' "/var/run/postgresql:5432:*:oo_a356_admin:${pass}" > "$pf"
chmod 600 "$pf"
export PGPASSFILE=$pf
export PGHOST=/var/run/postgresql
export PGUSER=oo_a356_admin
export PGPORT=5432
export PGCLIENTENCODING=UTF8
unset pass

psql_db() {
  local db=$1
  shift
  psql --no-password --no-psqlrc --tuples-only --no-align --quiet -v ON_ERROR_STOP=1 -d "$db" "$@"
}

note() { printf '%s\n' "$1" | tee -a "$OUT/result.txt" >/dev/null; }

psql_db postgres -c "SELECT version();" > "$OUT/version.txt"
note "version=$(cat "$OUT/version.txt")"

psql_db postgres <<'SQL'
SELECT pg_terminate_backend(pid)
FROM pg_stat_activity
WHERE datname = 'oo_a377_seqprobe' AND pid <> pg_backend_pid();
SQL
psql_db postgres -c "DROP DATABASE IF EXISTS oo_a377_seqprobe;"
psql_db postgres -c "CREATE DATABASE oo_a377_seqprobe;"

psql_db oo_a377_seqprobe <<'SQL'
CREATE SCHEMA shop;
CREATE TABLE shop.entry (id integer PRIMARY KEY);
INSERT INTO shop.entry (id) VALUES (1);
CREATE SEQUENCE shop.cut_seq AS bigint START WITH 41;
CREATE SEQUENCE shop.cached_seq AS bigint CACHE 10 START WITH 1;
CREATE SEQUENCE shop.owned_seq AS bigint START WITH 7;
ALTER SEQUENCE shop.owned_seq OWNED BY shop.entry.id;
CREATE SEQUENCE shop.inflight_seq AS bigint START WITH 3;
CREATE ROLE oo_a377_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE ROLE oo_a377_operator NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT;
GRANT oo_a377_owner TO oo_a377_operator;
CREATE ROLE oo_a377_stranger NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;
GRANT USAGE, UPDATE ON SEQUENCE shop.cut_seq TO oo_a377_stranger;
GRANT MAINTAIN, UPDATE ON TABLE shop.entry TO oo_a377_stranger;
SQL

state_of() {
  psql_db oo_a377_seqprobe -c "SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text || '|' || (SELECT relfilenode::text FROM pg_class WHERE oid = '$1'::regclass) FROM $1;"
}

note "baseline_cut=$(state_of shop.cut_seq)"
note "baseline_owned=$(state_of shop.owned_seq)"
note "baseline_cached=$(state_of shop.cached_seq)"

# Warm the cache in a committed transaction, then the backend keeps unused values.
psql_db oo_a377_seqprobe -c "SELECT nextval('shop.cached_seq');" > "$OUT/cached-first.txt"
note "cached_first_nextval=$(tr -d '[:space:]' < "$OUT/cached-first.txt")"
note "cached_after_first=$(state_of shop.cached_seq)"

pg_dump --no-password --schema=shop --format=plain --no-owner --no-privileges oo_a377_seqprobe > "$OUT/baseline.sql"
note "baseline_setval_cut=$(grep -F "setval('shop.cut_seq'" "$OUT/baseline.sql" | head -1)"
note "baseline_setval_owned=$(grep -F "setval('shop.owned_seq'" "$OUT/baseline.sql" | head -1)"
note "baseline_owned_by=$(grep -n -F 'OWNED BY' "$OUT/baseline.sql" | head -5 | tr '\n' ';')"

cat > "$OUT/holder.sql" <<'SQL'
\pset tuples_only on
\pset format unaligned
BEGIN ISOLATION LEVEL REPEATABLE READ;
ALTER SEQUENCE shop.cut_seq OWNED BY NONE;
ALTER SEQUENCE shop.cached_seq OWNED BY NONE;
ALTER SEQUENCE shop.owned_seq OWNED BY shop.entry.id;
SELECT 'state_cut|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text || '|' || (SELECT relfilenode::text FROM pg_class WHERE oid = 'shop.cut_seq'::regclass) FROM shop.cut_seq;
SELECT 'state_cached|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text || '|' || (SELECT relfilenode::text FROM pg_class WHERE oid = 'shop.cached_seq'::regclass) FROM shop.cached_seq;
SELECT 'state_owned|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text || '|' || (SELECT relfilenode::text FROM pg_class WHERE oid = 'shop.owned_seq'::regclass) FROM shop.owned_seq;
SELECT 'snap|' || pg_catalog.pg_export_snapshot();
SELECT 'lock|' || c.relname || '|' || l.mode || '|' || l.granted::text
FROM pg_locks l
JOIN pg_class c ON c.oid = l.relation
WHERE c.relname IN ('cut_seq', 'cached_seq', 'owned_seq')
  AND l.pid = pg_backend_pid()
  AND l.granted
ORDER BY c.relname, l.mode;
SELECT pg_sleep(30);
COMMIT;
SQL

psql --no-password --no-psqlrc -d oo_a377_seqprobe -f "$OUT/holder.sql" > "$OUT/holder.out" 2> "$OUT/holder.err" &
holder_pid=$!

seen=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  count=$(psql_db oo_a377_seqprobe -c "SELECT count(*) FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'shop' AND c.relname = 'cut_seq' AND l.granted AND l.mode = 'ShareRowExclusiveLock';")
  count=$(printf '%s' "$count" | tr -d '[:space:]')
  if [ "$count" = 1 ]; then
    seen=1
    break
  fi
  sleep 0.25
done
note "share_row_exclusive_seen=$seen"

if [ "$seen" != 1 ]; then
  note "holder_err=$(tr '\n' ' ' < "$OUT/holder.err")"
  kill "$holder_pid" 2>/dev/null || true
  wait "$holder_pid" 2>/dev/null || true
  cp "$OUT/result.txt" /run-in/seq-probe-result.txt
  exit 1
fi

run_expect_fail() {
  local name=$1
  local sql=$2
  set +e
  psql --no-password --no-psqlrc -v ON_ERROR_STOP=1 -d oo_a377_seqprobe -c "$sql" > "$OUT/${name}.out" 2> "$OUT/${name}.err"
  local rc=$?
  set -e
  printf '%s\n' "$rc" > "$OUT/${name}.exit"
  local st
  st=$(sed -n 's/.*ERROR:  *\([0-9A-Z][0-9A-Z][0-9A-Z][0-9A-Z][0-9A-Z]\).*/\1/p' "$OUT/${name}.err" | head -1)
  note "${name}_exit=$rc"
  note "${name}_sqlstate=${st:-none}"
}

run_expect_fail nextval "SET lock_timeout = '1s'; SELECT nextval('shop.cut_seq');"
run_expect_fail setval "SET lock_timeout = '1s'; SELECT setval('shop.cut_seq', 99, true);"
run_expect_fail cached_nextval "SET lock_timeout = '1s'; SELECT nextval('shop.cached_seq');"
note "during_cut=$(state_of shop.cut_seq)"
note "during_cached=$(state_of shop.cached_seq)"
note "during_owned=$(state_of shop.owned_seq)"

snap=$(sed -n 's/^snap|//p' "$OUT/holder.out" | head -1 | tr -d '[:space:]')
note "snapshot=$snap"
if [ -n "$snap" ]; then
  set +e
  pg_dump --no-password --snapshot="$snap" --schema=shop --format=plain --no-owner --no-privileges oo_a377_seqprobe > "$OUT/during.sql" 2> "$OUT/during-dump.err"
  dump_rc=$?
  set -e
  note "during_dump_exit=$dump_rc"
  note "during_setval_cut=$(grep -F "setval('shop.cut_seq'" "$OUT/during.sql" | head -1)"
  note "during_setval_cached=$(grep -F "setval('shop.cached_seq'" "$OUT/during.sql" | head -1)"
  note "during_setval_owned=$(grep -F "setval('shop.owned_seq'" "$OUT/during.sql" | head -1)"
  note "during_owned_by=$(grep -F 'OWNED BY' "$OUT/during.sql" | head -5 | tr '\n' ';')"
else
  note "during_dump_exit=missing_snapshot"
fi

wait "$holder_pid"
note "holder_exit=$?"
note "after_commit_cut=$(state_of shop.cut_seq)"
note "after_commit_cached=$(state_of shop.cached_seq)"

# After the fence drops, the cached backend must be a new backend to see whether
# THIS backend still cannot skip the lock. A fresh backend has an empty cache.
# The cache-bypass question is the nextval that ran while the lock was held.
psql_db oo_a377_seqprobe -c "SELECT nextval('shop.cached_seq');" > "$OUT/cached-after-release.txt"
note "cached_after_release_nextval=$(tr -d '[:space:]' < "$OUT/cached-after-release.txt")"
note "cached_after_release_state=$(state_of shop.cached_seq)"

# Same-value CACHE and INCREMENT rewrite the sequence tuple (log_cnt), then roll back.
psql_db oo_a377_seqprobe -c "SELECT nextval('shop.cut_seq');" > "$OUT/cut-one-next.txt"
note "cut_after_one_next=$(state_of shop.cut_seq)"
psql_db oo_a377_seqprobe <<'SQL' > "$OUT/rewrite-cache.txt"
BEGIN;
SELECT 'before|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text || '|' || (SELECT relfilenode::text FROM pg_class WHERE oid = 'shop.cut_seq'::regclass) FROM shop.cut_seq;
ALTER SEQUENCE shop.cut_seq CACHE 1;
SELECT 'after|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text || '|' || (SELECT relfilenode::text FROM pg_class WHERE oid = 'shop.cut_seq'::regclass) FROM shop.cut_seq;
ROLLBACK;
SQL
note "rewrite_cache=$(tr '\n' ';' < "$OUT/rewrite-cache.txt")"
psql_db oo_a377_seqprobe <<'SQL' > "$OUT/rewrite-increment.txt"
BEGIN;
SELECT 'before|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text || '|' || (SELECT relfilenode::text FROM pg_class WHERE oid = 'shop.cut_seq'::regclass) FROM shop.cut_seq;
ALTER SEQUENCE shop.cut_seq INCREMENT BY 1;
SELECT 'after|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text || '|' || (SELECT relfilenode::text FROM pg_class WHERE oid = 'shop.cut_seq'::regclass) FROM shop.cut_seq;
ROLLBACK;
SQL
note "rewrite_increment=$(tr '\n' ';' < "$OUT/rewrite-increment.txt")"
note "cut_after_rollback=$(state_of shop.cut_seq)"

# In-flight nextval holds RowExclusiveLock. ALTER must wait, not sneak past.
psql_db oo_a377_seqprobe -c "BEGIN; SELECT nextval('shop.inflight_seq'); SELECT pg_sleep(8);" > "$OUT/inflight.out" 2> "$OUT/inflight.err" &
inflight_pid=$!
sleep 1
run_expect_fail alter_while_inflight "SET lock_timeout = '1s'; ALTER SEQUENCE shop.inflight_seq OWNED BY NONE;"
wait "$inflight_pid" || true
note "inflight_state=$(state_of shop.inflight_seq)"

# Read-only nextval is rejected for this backend. It does not fence anyone else.
run_expect_fail readonly_nextval "BEGIN READ ONLY; SELECT nextval('shop.cut_seq');"

# Privilege: stranger with USAGE/UPDATE/MAINTAIN cannot ALTER. Operator who inherits the owner can.
psql_db oo_a377_seqprobe <<'SQL' > "$OUT/priv.txt" 2> "$OUT/priv.err" || true
BEGIN;
ALTER SEQUENCE shop.cut_seq OWNER TO oo_a377_owner;
ALTER TABLE shop.entry OWNER TO oo_a377_owner;
SET ROLE oo_a377_stranger;
LOCK TABLE shop.entry IN SHARE MODE;
ALTER SEQUENCE shop.cut_seq OWNED BY NONE;
SQL
note "priv_stranger_sqlstate=$(sed -n 's/.*ERROR:  *\([0-9A-Z][0-9A-Z][0-9A-Z][0-9A-Z][0-9A-Z]\).*/\1/p' "$OUT/priv.err" | head -1)"
note "priv_stranger_err=$(tr '\n' ' ' < "$OUT/priv.err")"

psql_db oo_a377_seqprobe <<'SQL' > "$OUT/priv-op.txt" 2> "$OUT/priv-op.err"
BEGIN;
ALTER SEQUENCE shop.cut_seq OWNER TO oo_a377_owner;
ALTER TABLE shop.entry OWNER TO oo_a377_owner;
GRANT USAGE ON SCHEMA shop TO oo_a377_operator;
SET ROLE oo_a377_operator;
LOCK TABLE shop.entry IN SHARE MODE;
ALTER SEQUENCE shop.cut_seq OWNED BY NONE;
SELECT 'op_state|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM shop.cut_seq;
SELECT 'op_lock|' || l.mode || '|' || l.granted::text
FROM pg_locks l
JOIN pg_class c ON c.oid = l.relation
WHERE c.relname = 'cut_seq' AND l.pid = pg_backend_pid() AND l.granted
ORDER BY l.mode;
ROLLBACK;
SQL
note "priv_operator_exit=0"
note "priv_operator=$(tr '\n' ';' < "$OUT/priv-op.txt")"

note "holder_lines_begin"
cat "$OUT/holder.out" >> "$OUT/result.txt"
note "holder_lines_end"
cp "$OUT/result.txt" /run-in/seq-probe-result.txt
cp "$OUT/during.sql" /run-in/seq-probe-during.sql 2>/dev/null || true
cp "$OUT/holder.err" /run-in/seq-probe-holder.err 2>/dev/null || true
printf '%s\n' done > /run-in/seq-probe.exit
