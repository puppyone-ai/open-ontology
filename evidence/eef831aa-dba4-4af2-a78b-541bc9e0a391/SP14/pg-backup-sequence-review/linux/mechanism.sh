#!/bin/bash
# Private PostgreSQL 17.10 measurements of ALTER SEQUENCE OWNED BY.
# Run by run-live.sh. Does not start PostgreSQL or a Rust builder.
# The database oo_a391_mech is not a product source database.
set -uo pipefail

if [ -z "${PGNAME:-}" ] || [ -z "${ROOT:-}" ] || [ -z "${EVIDENCE:-}" ] || [ -z "${REAL_TOOLDIR:-}" ]; then
  echo "mechanism.sh is run by run-live.sh" >&2
  exit 2
fi

MECH=${EVIDENCE}/mechanism
mkdir -p "$MECH"
: > "$MECH/results.txt"
fail=0

note() {
  printf '%s\n' "$1" | tee -a "$MECH/results.txt"
}

mark() {
  name=$1
  ok=$2
  detail=$3
  if [ "$ok" = yes ]; then
    note "pass ${name} ${detail}"
  else
    note "fail ${name} ${detail}"
    fail=1
  fi
}

sql_file() {
  db=$1
  name=$2
  dest=$3
  set +e
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/psql-exec.sh "$db" "$name" > "$dest" 2> "${dest}.err"
  code=$?
  set +e
  printf '%s\n' "$code" > "${dest}.exit"
  echo "$code"
}

write_sql() {
  name=$1
  body=$2
  printf '%s\n' "$body" > "$ROOT/run/${name}"
}

note "pg_source=postgres/postgres REL_17_10 src/backend/commands/sequence.c"
note "alter_lock=ShareRowExclusiveLock at AlterSequence RangeVarGetRelidExtended"
note "owned_by_skips_need_seq_rewrite=init_params owned_by branch does not set the flag"
note "rewrite_comment=OWNED BY must not change relfilenumber, lines 1248-1254"
note "identity_sqlstate=0A000 cannot change ownership of identity sequence, process_owned_by"
note "cache_lock=init_sequence calls lock_and_open_sequence before dispensing SeqTable cache"

write_sql m-create.sql "CREATE DATABASE oo_a391_mech TEMPLATE template0 ENCODING 'UTF8';"
sql_file postgres m-create.sql "$MECH/create.out" >/dev/null
mark create_db "$([ "$(cat "$MECH/create.out.exit")" = 0 ] && echo yes || echo no)" "exit=$(cat "$MECH/create.out.exit")"

write_sql m-setup.sql "$(cat <<'EOF'
CREATE SCHEMA mech;
CREATE TABLE mech.item (id bigint PRIMARY KEY);
CREATE SEQUENCE mech.plain AS bigint START WITH 41 INCREMENT BY 1;
CREATE SEQUENCE mech.owned AS bigint START WITH 41 INCREMENT BY 1 OWNED BY mech.item.id;
CREATE SEQUENCE mech.cached AS bigint START WITH 1 INCREMENT BY 1 CACHE 10;
CREATE TABLE mech.ident_row (id bigint GENERATED ALWAYS AS IDENTITY);
CREATE ROLE oo_a391_owner NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS;
CREATE ROLE oo_a391_operator NOLOGIN INHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS;
CREATE ROLE oo_a391_stranger NOLOGIN NOINHERIT NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS;
GRANT oo_a391_owner TO oo_a391_operator;
ALTER SCHEMA mech OWNER TO oo_a391_owner;
ALTER TABLE mech.item OWNER TO oo_a391_owner;
ALTER TABLE mech.ident_row OWNER TO oo_a391_owner;
ALTER SEQUENCE mech.plain OWNER TO oo_a391_owner;
ALTER SEQUENCE mech.owned OWNER TO oo_a391_owner;
ALTER SEQUENCE mech.cached OWNER TO oo_a391_owner;
GRANT USAGE, UPDATE ON SEQUENCE mech.plain TO oo_a391_stranger;
EOF
)"
sql_file oo_a391_mech m-setup.sql "$MECH/setup.out" >/dev/null
mark setup "$([ "$(cat "$MECH/setup.out.exit")" = 0 ] && echo yes || echo no)" "exit=$(cat "$MECH/setup.out.exit")"

write_sql m-state.sql "$(cat <<'EOF'
SELECT 'plain|' || c.relfilenode::text || '|' || s.last_value::text || '|' || CASE WHEN s.is_called THEN 't' ELSE 'f' END || '|' || s.log_cnt::text || '|xmin|' || q.xmin::text
FROM mech.plain s
JOIN pg_catalog.pg_class c ON c.oid = 'mech.plain'::regclass
JOIN pg_catalog.pg_sequence q ON q.seqrelid = c.oid;
SELECT 'owned|' || c.relfilenode::text || '|' || s.last_value::text || '|' || CASE WHEN s.is_called THEN 't' ELSE 'f' END || '|' || s.log_cnt::text || '|xmin|' || q.xmin::text
FROM mech.owned s
JOIN pg_catalog.pg_class c ON c.oid = 'mech.owned'::regclass
JOIN pg_catalog.pg_sequence q ON q.seqrelid = c.oid;
SELECT 'depend|' || n.nspname || '.' || c.relname || '|' || a.attname || '|' || d.deptype
FROM pg_catalog.pg_depend d
JOIN pg_catalog.pg_class seq ON seq.oid = d.objid
JOIN pg_catalog.pg_namespace seqns ON seqns.oid = seq.relnamespace
JOIN pg_catalog.pg_class c ON c.oid = d.refobjid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = d.refobjsubid
WHERE d.classid = 'pg_catalog.pg_class'::regclass
  AND d.refclassid = 'pg_catalog.pg_class'::regclass
  AND d.deptype IN ('a','i')
  AND seqns.nspname = 'mech'
ORDER BY 1;
SELECT 'identseq|' || n.nspname || '.' || c.relname
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE c.relkind = 'S' AND n.nspname = 'mech' AND c.relname LIKE '%ident%'
ORDER BY 1;
SELECT 'roles|' || rolname || '|' || CASE WHEN rolsuper THEN 't' ELSE 'f' END || '|' || CASE WHEN rolcreaterole THEN 't' ELSE 'f' END || '|' || CASE WHEN rolcreatedb THEN 't' ELSE 'f' END || '|' || CASE WHEN rolinherit THEN 't' ELSE 'f' END
FROM pg_catalog.pg_roles
WHERE rolname IN ('oo_a391_owner','oo_a391_operator','oo_a391_stranger','oo_a356_admin')
ORDER BY 1;
EOF
)"
sql_file oo_a391_mech m-state.sql "$MECH/before.txt" >/dev/null
note "before $(tr '\n' ' ' < "$MECH/before.txt")"

write_sql m-owned-rollback.sql "$(cat <<'EOF'
BEGIN;
ALTER SEQUENCE mech.plain OWNED BY NONE;
SELECT 'during-plain|' || c.relfilenode::text || '|' || s.last_value::text || '|' || CASE WHEN s.is_called THEN 't' ELSE 'f' END || '|' || s.log_cnt::text || '|xmin|' || q.xmin::text
FROM mech.plain s
JOIN pg_catalog.pg_class c ON c.oid = 'mech.plain'::regclass
JOIN pg_catalog.pg_sequence q ON q.seqrelid = c.oid;
ALTER SEQUENCE mech.owned OWNED BY mech.item.id;
SELECT 'during-owned|' || c.relfilenode::text || '|' || s.last_value::text || '|' || CASE WHEN s.is_called THEN 't' ELSE 'f' END || '|' || s.log_cnt::text || '|xmin|' || q.xmin::text
FROM mech.owned s
JOIN pg_catalog.pg_class c ON c.oid = 'mech.owned'::regclass
JOIN pg_catalog.pg_sequence q ON q.seqrelid = c.oid;
ROLLBACK;
SELECT 'after-plain|' || c.relfilenode::text || '|' || s.last_value::text || '|' || CASE WHEN s.is_called THEN 't' ELSE 'f' END || '|' || s.log_cnt::text || '|xmin|' || q.xmin::text
FROM mech.plain s
JOIN pg_catalog.pg_class c ON c.oid = 'mech.plain'::regclass
JOIN pg_catalog.pg_sequence q ON q.seqrelid = c.oid;
SELECT 'after-owned|' || c.relfilenode::text || '|' || s.last_value::text || '|' || CASE WHEN s.is_called THEN 't' ELSE 'f' END || '|' || s.log_cnt::text || '|xmin|' || q.xmin::text
FROM mech.owned s
JOIN pg_catalog.pg_class c ON c.oid = 'mech.owned'::regclass
JOIN pg_catalog.pg_sequence q ON q.seqrelid = c.oid;
EOF
)"
sql_file oo_a391_mech m-owned-rollback.sql "$MECH/owned-rollback.txt" >/dev/null
plain_before=$(grep '^plain|' "$MECH/before.txt" | head -1)
owned_before=$(grep '^owned|' "$MECH/before.txt" | head -1)
during_plain=$(grep '^during-plain|' "$MECH/owned-rollback.txt" | head -1)
during_owned=$(grep '^during-owned|' "$MECH/owned-rollback.txt" | head -1)
after_plain=$(grep '^after-plain|' "$MECH/owned-rollback.txt" | head -1)
after_owned=$(grep '^after-owned|' "$MECH/owned-rollback.txt" | head -1)
plain_tail=${plain_before#plain|}
owned_tail=${owned_before#owned|}
plain_data=${plain_tail%%|xmin|*}
owned_data=${owned_tail%%|xmin|*}
during_plain_body=${during_plain#during-plain|}
during_owned_body=${during_owned#during-owned|}
after_plain_body=${after_plain#after-plain|}
after_owned_body=${after_owned#after-owned|}
during_plain_data=${during_plain_body%%|xmin|*}
during_owned_data=${during_owned_body%%|xmin|*}
after_plain_data=${after_plain_body%%|xmin|*}
after_owned_data=${after_owned_body%%|xmin|*}
mark owned_by_plain_data_unchanged "$([ -n "$plain_data" ] && [ "$during_plain_data" = "$plain_data" ] && [ "$after_plain_data" = "$plain_data" ] && echo yes || echo no)" "data=${plain_data} during=${during_plain_data} after=${after_plain_data}"
mark owned_by_column_data_unchanged "$([ -n "$owned_data" ] && [ "$during_owned_data" = "$owned_data" ] && [ "$after_owned_data" = "$owned_data" ] && echo yes || echo no)" "data=${owned_data} during=${during_owned_data} after=${after_owned_data}"
note "catalog_xmin plain before=${plain_tail##*|xmin|} during=${during_plain_body##*|xmin|} after=${after_plain_body##*|xmin|}"
note "catalog_xmin owned before=${owned_tail##*|xmin|} during=${during_owned_body##*|xmin|} after=${after_owned_body##*|xmin|}"

write_sql m-rewrite-rollback.sql "$(cat <<'EOF'
BEGIN;
SELECT 'cache-before|' || relfilenode::text FROM pg_catalog.pg_class WHERE oid = 'mech.plain'::regclass;
ALTER SEQUENCE mech.plain CACHE 1;
SELECT 'cache-during|' || c.relfilenode::text || '|' || s.last_value::text || '|' || CASE WHEN s.is_called THEN 't' ELSE 'f' END || '|' || s.log_cnt::text
FROM mech.plain s JOIN pg_catalog.pg_class c ON c.oid = 'mech.plain'::regclass;
ROLLBACK;
SELECT 'cache-after|' || relfilenode::text FROM pg_catalog.pg_class WHERE oid = 'mech.plain'::regclass;
BEGIN;
SELECT 'inc-before|' || relfilenode::text FROM pg_catalog.pg_class WHERE oid = 'mech.plain'::regclass;
ALTER SEQUENCE mech.plain INCREMENT BY 1;
SELECT 'inc-during|' || c.relfilenode::text || '|' || s.log_cnt::text
FROM mech.plain s JOIN pg_catalog.pg_class c ON c.oid = 'mech.plain'::regclass;
ROLLBACK;
SELECT 'inc-after|' || relfilenode::text FROM pg_catalog.pg_class WHERE oid = 'mech.plain'::regclass;
SELECT 'data-after|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM mech.plain;
EOF
)"
sql_file oo_a391_mech m-rewrite-rollback.sql "$MECH/rewrite-rollback.txt" >/dev/null
cache_before=$(grep '^cache-before|' "$MECH/rewrite-rollback.txt" | head -1 | cut -d'|' -f2)
cache_during_node=$(grep '^cache-during|' "$MECH/rewrite-rollback.txt" | head -1 | cut -d'|' -f2)
cache_after=$(grep '^cache-after|' "$MECH/rewrite-rollback.txt" | head -1 | cut -d'|' -f2)
inc_before=$(grep '^inc-before|' "$MECH/rewrite-rollback.txt" | head -1 | cut -d'|' -f2)
inc_during_node=$(grep '^inc-during|' "$MECH/rewrite-rollback.txt" | head -1 | cut -d'|' -f2)
inc_after=$(grep '^inc-after|' "$MECH/rewrite-rollback.txt" | head -1 | cut -d'|' -f2)
data_after=$(grep '^data-after|' "$MECH/rewrite-rollback.txt" | head -1)
mark cache_rewrites_relfilenode "$([ -n "$cache_before" ] && [ "$cache_during_node" != "$cache_before" ] && echo yes || echo no)" "before=${cache_before} during=${cache_during_node}"
mark cache_rollback_restores "$([ "$cache_before" = "$cache_after" ] && echo yes || echo no)" "before=${cache_before} after=${cache_after}"
mark increment_rewrites_relfilenode "$([ -n "$inc_before" ] && [ "$inc_during_node" != "$inc_before" ] && echo yes || echo no)" "before=${inc_before} during=${inc_during_node}"
mark increment_rollback_restores "$([ "$inc_before" = "$inc_after" ] && [ "$data_after" = "data-after|41|f|0" ] && echo yes || echo no)" "after=${inc_after} data=${data_after}"

write_sql m-lock-table.sql "$(cat <<'EOF'
DO $lock$
BEGIN
  EXECUTE 'LOCK TABLE mech.plain';
  RAISE EXCEPTION 'lock table accepted';
EXCEPTION
  WHEN wrong_object_type THEN
    RAISE NOTICE 'locktable %', SQLSTATE;
END
$lock$;
EOF
)"
sql_file oo_a391_mech m-lock-table.sql "$MECH/lock-table.out" >/dev/null
if grep -q 'locktable 42809' "$MECH/lock-table.out" "$MECH/lock-table.out.err"; then
  mark lock_table_42809 yes "42809"
else
  mark lock_table_42809 no "see lock-table.out.err"
fi

write_sql m-identity.sql "$(cat <<'EOF'
DO $id$
DECLARE
  seq text;
  st text;
  msg text;
  data text;
BEGIN
  SELECT format('%I.%I', n.nspname, c.relname) INTO seq
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  WHERE c.relkind = 'S' AND n.nspname = 'mech' AND c.relname LIKE '%ident%';
  IF seq IS NULL THEN
    RAISE NOTICE 'identity-none missing';
    RAISE NOTICE 'identity-same missing';
    RETURN;
  END IF;
  RAISE NOTICE 'identity-name %', seq;
  BEGIN
    EXECUTE format('ALTER SEQUENCE %s OWNED BY NONE', seq);
    RAISE EXCEPTION 'identity none accepted';
  EXCEPTION WHEN OTHERS THEN
    GET STACKED DIAGNOSTICS st = RETURNED_SQLSTATE, msg = MESSAGE_TEXT;
    RAISE NOTICE 'identity-none % %', st, msg;
  END;
  BEGIN
    EXECUTE format('ALTER SEQUENCE %s OWNED BY mech.ident_row.id', seq);
    RAISE EXCEPTION 'identity same accepted';
  EXCEPTION WHEN OTHERS THEN
    GET STACKED DIAGNOSTICS st = RETURNED_SQLSTATE, msg = MESSAGE_TEXT;
    RAISE NOTICE 'identity-same % %', st, msg;
  END;
  EXECUTE format($q$SELECT 'ident-data|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM %s$q$, seq) INTO data;
  RAISE NOTICE '%', data;
END
$id$;
EOF
)"
sql_file oo_a391_mech m-identity.sql "$MECH/identity.out" >/dev/null
if grep -q 'identity-none 0A000' "$MECH/identity.out" "$MECH/identity.out.err" && grep -q 'identity-same 0A000' "$MECH/identity.out" "$MECH/identity.out.err"; then
  mark identity_owned_by_0A000 yes "0A000"
else
  mark identity_owned_by_0A000 no "see identity.out.err"
fi

write_sql m-priv.sql "$(cat <<'EOF'
SELECT 'operator-super|' || CASE WHEN rolsuper THEN 't' ELSE 'f' END FROM pg_catalog.pg_roles WHERE rolname = 'oo_a391_operator';
DO $stranger$
BEGIN
  EXECUTE 'SET LOCAL ROLE oo_a391_stranger';
  EXECUTE 'ALTER SEQUENCE mech.plain OWNED BY NONE';
  RAISE EXCEPTION 'stranger alter accepted';
EXCEPTION WHEN insufficient_privilege THEN
  RAISE NOTICE 'stranger 42501';
END
$stranger$;
BEGIN;
SET LOCAL ROLE oo_a391_operator;
SELECT 'operator-user|' || current_user || '|' || CASE WHEN rolsuper THEN 't' ELSE 'f' END
FROM pg_catalog.pg_roles WHERE rolname = current_user;
ALTER SEQUENCE mech.plain OWNED BY NONE;
SELECT 'operator-state|' || c.relfilenode::text || '|' || s.last_value::text || '|' || CASE WHEN s.is_called THEN 't' ELSE 'f' END || '|' || s.log_cnt::text
FROM mech.plain s JOIN pg_catalog.pg_class c ON c.oid = 'mech.plain'::regclass;
ROLLBACK;
EOF
)"
sql_file oo_a391_mech m-priv.sql "$MECH/priv.out" >/dev/null
op_super=$(grep '^operator-super|' "$MECH/priv.out" | head -1 | cut -d'|' -f2)
op_user=$(grep '^operator-user|' "$MECH/priv.out" | head -1)
op_state=$(grep '^operator-state|' "$MECH/priv.out" | head -1)
stranger_ok=no
grep -q 'stranger 42501' "$MECH/priv.out" "$MECH/priv.out.err" && stranger_ok=yes
mark stranger_cannot_alter "$stranger_ok" "42501"
mark operator_not_super "$([ "$op_super" = f ] && echo yes || echo no)" "rolsuper=${op_super} ${op_user}"
mark operator_owned_by_unchanged "$([ "$op_state" = "operator-state|${plain_data}" ] && echo yes || echo no)" "${op_state} expected=${plain_data}"

write_sql m-grant-super.sql "GRANT oo_a356_admin TO oo_a391_operator;"
set +e
docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/psql-exec.sh postgres m-grant-super.sql > "$MECH/grant-super.out" 2> "$MECH/grant-super.err"
grant_code=$?
set +e
printf '%s\n' "$grant_code" > "$MECH/grant-super.exit"
note "grant_superuser_role exit=${grant_code}"
if [ "$grant_code" = 0 ]; then
  write_sql m-revoke-super.sql "REVOKE oo_a356_admin FROM oo_a391_operator;"
  sql_file postgres m-revoke-super.sql "$MECH/revoke-super.out" >/dev/null
  note "grant_superuser_role revoked exit=$(cat "$MECH/revoke-super.out.exit")"
fi

write_sql m-cache-session.sql "$(cat <<'EOF'
\set VERBOSITY verbose
SELECT 'primed|' || nextval('mech.cached')::text;
SELECT 'disk|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM mech.cached;
SET lock_timeout = '2s';
DO $wait$
DECLARE
  n int := 0;
  i int;
BEGIN
  FOR i IN 1..200 LOOP
    SELECT count(*) INTO n
    FROM pg_catalog.pg_locks l
    JOIN pg_catalog.pg_class c ON c.oid = l.relation
    JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
    WHERE ns.nspname = 'mech'
      AND c.relname = 'cached'
      AND l.granted
      AND l.mode = 'ShareRowExclusiveLock';
    EXIT WHEN n >= 1;
    PERFORM pg_sleep(0.1);
  END LOOP;
  IF n < 1 THEN
    RAISE EXCEPTION 'cached fence was not visible';
  END IF;
  PERFORM nextval('mech.cached');
  RAISE EXCEPTION 'cached nextval committed';
EXCEPTION
  WHEN lock_not_available THEN
    RAISE EXCEPTION '55P03 cached-nextval';
END
$wait$;
EOF
)"
write_sql m-setval-session.sql "$(cat <<'EOF'
\set VERBOSITY verbose
SET lock_timeout = '2s';
DO $wait$
DECLARE
  n int := 0;
  i int;
BEGIN
  FOR i IN 1..200 LOOP
    SELECT count(*) INTO n
    FROM pg_catalog.pg_locks l
    JOIN pg_catalog.pg_class c ON c.oid = l.relation
    JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
    WHERE ns.nspname = 'mech'
      AND c.relname = 'cached'
      AND l.granted
      AND l.mode = 'ShareRowExclusiveLock';
    EXIT WHEN n >= 1;
    PERFORM pg_sleep(0.1);
  END LOOP;
  IF n < 1 THEN
    RAISE EXCEPTION 'cached fence was not visible';
  END IF;
  PERFORM setval('mech.cached', 99, true);
  RAISE EXCEPTION 'cached setval committed';
EXCEPTION
  WHEN lock_not_available THEN
    RAISE EXCEPTION '55P03 cached-setval';
END
$wait$;
EOF
)"
write_sql m-hold.sql "$(cat <<'EOF'
BEGIN;
ALTER SEQUENCE mech.cached OWNED BY NONE;
SELECT pg_sleep(25);
COMMIT;
EOF
)"

docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/psql-exec.sh oo_a391_mech m-cache-session.sql > "$MECH/cache-session.out" 2> "$MECH/cache-session.err" &
cache_pid=$!
primed=0
for _try in $(seq 1 50); do
  if grep -q '^primed|' "$MECH/cache-session.out" 2>/dev/null && grep -q '^disk|' "$MECH/cache-session.out" 2>/dev/null; then
    primed=1
    break
  fi
  if ! kill -0 "$cache_pid" 2>/dev/null; then
    break
  fi
  sleep 0.2
done
mark cache_prime "$([ "$primed" = 1 ] && echo yes || echo no)" "$(grep '^primed|' "$MECH/cache-session.out" 2>/dev/null || true) $(grep '^disk|' "$MECH/cache-session.out" 2>/dev/null || true)"
disk_before=$(grep '^disk|' "$MECH/cache-session.out" | head -1)

docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/psql-exec.sh oo_a391_mech m-hold.sql > "$MECH/hold.out" 2> "$MECH/hold.err" &
hold_pid=$!
docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/psql-exec.sh oo_a391_mech m-setval-session.sql > "$MECH/setval-session.out" 2> "$MECH/setval-session.err" &
setval_pid=$!

seen_lock=0
for _try in $(seq 1 50); do
  write_sql m-locks.sql "SELECT mode || '|' || CASE WHEN granted THEN 't' ELSE 'f' END FROM pg_catalog.pg_locks l JOIN pg_catalog.pg_class c ON c.oid = l.relation JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = 'mech' AND c.relname = 'cached' ORDER BY 1;"
  sql_file oo_a391_mech m-locks.sql "$MECH/locks-during.txt" >/dev/null || true
  if grep -q 'ShareRowExclusiveLock|t' "$MECH/locks-during.txt"; then
    seen_lock=1
    break
  fi
  sleep 0.2
done
cp "$MECH/locks-during.txt" "$MECH/locks-seen.txt" 2>/dev/null || true
mark share_row_exclusive_visible "$([ "$seen_lock" = 1 ] && echo yes || echo no)" "$(tr '\n' ' ' < "$MECH/locks-during.txt" 2>/dev/null || true)"

wait "$cache_pid" || true
wait "$setval_pid" || true
printf '%s\n' "$?" > "$MECH/setval-wait.exit"
cache_rc=0
grep -q '55P03 cached-nextval' "$MECH/cache-session.err" || cache_rc=1
setval_rc=0
grep -q '55P03 cached-setval' "$MECH/setval-session.err" || setval_rc=1
mark cached_nextval_blocked "$([ "$cache_rc" = 0 ] && echo yes || echo no)" "stderr-has-55P03"
mark cached_setval_blocked "$([ "$setval_rc" = 0 ] && echo yes || echo no)" "stderr-has-55P03"

write_sql m-disk-during.sql "SELECT 'during|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM mech.cached;"
sql_file oo_a391_mech m-disk-during.sql "$MECH/disk-during.txt" >/dev/null
during_disk=$(grep '^during|' "$MECH/disk-during.txt" | head -1)
mark cached_disk_unchanged "$([ "$during_disk" = "during|${disk_before#disk|}" ] && echo yes || echo no)" "before=${disk_before} during=${during_disk}"

wait "$hold_pid" || true
printf '%s\n' 0 > "$MECH/hold.exit"

write_sql m-after-release.sql "$(cat <<'EOF'
SELECT 'still|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM mech.cached;
SELECT 'usable|' || nextval('mech.cached')::text;
SELECT 'after-use|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM mech.cached;
SELECT 'plain-still|' || last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM mech.plain;
EOF
)"
sql_file oo_a391_mech m-after-release.sql "$MECH/after-release.txt" >/dev/null
still=$(grep '^still|' "$MECH/after-release.txt" | head -1)
usable=$(grep '^usable|' "$MECH/after-release.txt" | head -1)
plain_still=$(grep '^plain-still|' "$MECH/after-release.txt" | head -1)
mark release_keeps_disk "$([ "$still" = "still|${disk_before#disk|}" ] && echo yes || echo no)" "${still}"
mark release_nextval_usable "$([ "$(cat "$MECH/after-release.txt.exit")" = 0 ] && [ -n "$usable" ] && echo yes || echo no)" "${usable}"
mark plain_still_41 "$([ "$plain_still" = "plain-still|41|f|0" ] && echo yes || echo no)" "${plain_still}"

write_sql m-dump-hold.sql "$(cat <<'EOF'
BEGIN;
ALTER SEQUENCE mech.plain OWNED BY NONE;
SELECT pg_sleep(20);
COMMIT;
EOF
)"
docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/psql-exec.sh oo_a391_mech m-dump-hold.sql > "$MECH/dump-hold.out" 2> "$MECH/dump-hold.err" &
dump_hold_pid=$!
dump_ready=0
for _try in $(seq 1 40); do
  write_sql m-plain-lock.sql "SELECT count(*)::text FROM pg_catalog.pg_locks l JOIN pg_catalog.pg_class c ON c.oid = l.relation WHERE c.oid = 'mech.plain'::regclass AND l.granted AND l.mode = 'ShareRowExclusiveLock';"
  sql_file oo_a391_mech m-plain-lock.sql "$MECH/plain-lock.txt" >/dev/null || true
  if [ "$(tr -d '[:space:]' < "$MECH/plain-lock.txt")" = 1 ]; then
    dump_ready=1
    break
  fi
  sleep 0.2
done
mark dump_lock_visible "$([ "$dump_ready" = 1 ] && echo yes || echo no)" "count=$(tr -d '[:space:]' < "$MECH/plain-lock.txt" 2>/dev/null || true)"

cat > "$ROOT/run/m-dump.sh" <<EOF
#!/bin/bash
set -euo pipefail
umask 077
passfile=\$(mktemp /tmp/a391.dump.XXXXXX)
trap 'rm -f "\$passfile"' EXIT
pass=\$(tr -d '\\r\\n' < /var/lib/postgresql/a391-admin.pass)
printf '%s\\n' "/var/run/postgresql:5432:*:oo_a356_admin:\${pass}" > "\$passfile"
chmod 600 "\$passfile"
export PGPASSFILE=\$passfile
export PGHOST=/var/run/postgresql
export PGUSER=oo_a356_admin
export PGPORT=5432
${REAL_TOOLDIR}/pg_dump --format=custom --no-password --encoding=UTF8 --file=/var/lib/postgresql/a391-work/mech.dump --dbname=oo_a391_mech
${REAL_TOOLDIR}/pg_restore -f /var/lib/postgresql/a391-work/mech.sql /var/lib/postgresql/a391-work/mech.dump
EOF
docker cp "$ROOT/run/m-dump.sh" "$PGNAME":/var/lib/postgresql/a391-work/m-dump.sh
docker exec -u 0 "$PGNAME" chmod 755 /var/lib/postgresql/a391-work/m-dump.sh
set +e
docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/m-dump.sh > "$MECH/dump.out" 2> "$MECH/dump.err"
dump_code=$?
set +e
printf '%s\n' "$dump_code" > "$MECH/dump.exit"
docker cp "$PGNAME":/var/lib/postgresql/a391-work/mech.sql "$MECH/mech.sql" || true
wait "$dump_hold_pid" || true
if [ -f "$MECH/mech.sql" ]; then
  grep -n -E 'CREATE TABLE|CREATE SEQUENCE|setval|OWNED BY|--schema-only' "$MECH/mech.sql" > "$MECH/dump-lines.txt" || true
else
  : > "$MECH/dump-lines.txt"
fi
has_table=no
has_setval=no
has_schema_only=no
grep -q 'CREATE TABLE mech.item' "$MECH/mech.sql" 2>/dev/null && has_table=yes
grep -q "setval('mech.plain'" "$MECH/mech.sql" 2>/dev/null && has_setval=yes
grep -q -- '--schema-only' "$MECH/dump.out" "$MECH/dump.err" && has_schema_only=yes
mark dump_during_lock "$([ "$dump_code" = 0 ] && [ "$has_table" = yes ] && [ "$has_setval" = yes ] && [ "$has_schema_only" = no ] && echo yes || echo no)" "exit=${dump_code} table=${has_table} setval=${has_setval}"

printf '%s\n' "$fail" > "$MECH/assert.exit"
note "assert_exit=${fail}"
if [ "$fail" -ne 0 ]; then
  echo "mechanism assertions failed" >&2
fi
exit "$fail"
