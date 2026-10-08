#!/bin/bash
# Full 100k logical backup on the frozen source databases.
# Sourced by run-live.sh. Does not start PostgreSQL or a Rust builder.

if [ -z "${PGNAME:-}" ] || [ -z "${BIN:-}" ] || [ -z "${OPERATOR:-}" ] || [ -z "${REAL_TOOLDIR:-}" ]; then
  echo "live-phases.sh is sourced by run-live.sh" >&2
  exit 2
fi

LIVE=${EVIDENCE}/live
mkdir -p "$LIVE/phases" "$LIVE/boundary" "$LIVE/check-input/dumps" "$LIVE/check-input/probes"

phase() {
  name=$1
  expect=$2
  shift 2
  set +e
  invoke "$@" > "$LIVE/phases/${name}.out" 2> "$LIVE/phases/${name}.err"
  code=$?
  set -e
  printf '%s\n' "$code" > "$LIVE/phases/${name}.exit"
  echo "phase ${name} exit ${code}"
  if [ "$code" -ne "$expect" ]; then
    echo "phase ${name} expected ${expect}" >&2
    exit 1
  fi
}

sql_out() {
  db=$1
  dest=$2
  sql=$3
  name=$(basename "$dest").sql
  name=${name//./-}
  printf '%s\n' "$sql" > "$ROOT/run/${name}"
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/psql-exec.sh "$db" "$name" > "$dest"
}

sql_bg() {
  db=$1
  sqlname=$2
  dest=$3
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/psql-exec.sh "$db" "$sqlname" > "${dest}.out" 2> "${dest}.err" &
  echo $! > "${dest}.pid"
}

none_if_empty() {
  if [ ! -s "$1" ] || [ -z "$(tr -d '[:space:]' < "$1")" ]; then
    printf '%s\n' none > "$1"
  fi
}

seq_state_sql() {
  rel=$1
  label=$2
  printf "SELECT '%s|' || c.relfilenode::text || '|' || s.last_value::text || '|' || CASE WHEN s.is_called THEN 't' ELSE 'f' END || '|' || s.log_cnt::text FROM %s s JOIN pg_catalog.pg_class c ON c.oid = '%s'::regclass;" "$label" "$rel" "$rel"
}

docker cp "$EVIDENCE/linux/psql-exec.sh" "$PGNAME":/var/lib/postgresql/a391-work/psql-exec.sh
docker exec -u 0 "$PGNAME" chmod 755 /var/lib/postgresql/a391-work/psql-exec.sh

phase load-fixture 0 load-fixture "$OPERATOR"

sql_out oo_a356_agent "$LIVE/boundary/seq-before-agent.txt" "$(seq_state_sql shop.cut_seq agent)"
sql_out oo_a356_control "$LIVE/boundary/seq-before-control.txt" "$(seq_state_sql oo_key_fixture.cut_seq control)"
sql_out oo_a356_agent "$LIVE/boundary/identity-before-agent.txt" "SELECT count(*)::text FROM pg_catalog.pg_depend d JOIN pg_catalog.pg_class c ON c.oid = d.objid WHERE d.deptype = 'i' AND c.relkind = 'S'"
sql_out oo_a356_control "$LIVE/boundary/identity-before-control.txt" "SELECT count(*)::text FROM pg_catalog.pg_depend d JOIN pg_catalog.pg_class c ON c.oid = d.objid WHERE d.deptype = 'i' AND c.relkind = 'S'"
sql_out postgres "$LIVE/boundary/roles.txt" "SELECT rolname || '|' || CASE WHEN rolsuper THEN 't' ELSE 'f' END || '|' || CASE WHEN rolinherit THEN 't' ELSE 'f' END || '|' || CASE WHEN rolcreaterole THEN 't' ELSE 'f' END FROM pg_catalog.pg_roles WHERE rolname IN ('oo_a377_owner','oo_a377_operator','oo_a356_admin') ORDER BY 1"
sql_out postgres "$LIVE/boundary/members.txt" "SELECT r.rolname || ' member_of ' || g.rolname FROM pg_catalog.pg_auth_members m JOIN pg_catalog.pg_roles r ON r.oid = m.member JOIN pg_catalog.pg_roles g ON g.oid = m.roleid WHERE r.rolname = 'oo_a377_operator'"
sql_out oo_a356_agent "$LIVE/boundary/owners-agent.txt" "SELECT n.nspname || '.' || c.relname || '|' || pg_get_userbyid(c.relowner) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE c.relname = 'cut_seq'"
sql_out oo_a356_control "$LIVE/boundary/owners-control.txt" "SELECT n.nspname || '.' || c.relname || '|' || pg_get_userbyid(c.relowner) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE c.relname = 'cut_seq'"

cat > "$ROOT/run/q-reject.sql" <<'EOF'
\set VERBOSITY verbose
SET lock_timeout = '2s';
DO $probe$
DECLARE
  n int := 0;
  i int;
BEGIN
  FOR i IN 1..300 LOOP
    SELECT count(*) INTO n
    FROM pg_catalog.pg_locks l
    JOIN pg_catalog.pg_class c ON c.oid = l.relation
    JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
    JOIN pg_catalog.pg_stat_activity a ON a.pid = l.pid
    WHERE a.application_name = 'oo_a356_cut_agent'
      AND a.datname = 'oo_a356_agent'
      AND l.granted AND l.mode = 'ShareLock'
      AND ns.nspname = 'shop' AND c.relname = 'entry';
    EXIT WHEN n >= 1;
    PERFORM pg_sleep(0.1);
  END LOOP;
  IF n < 1 THEN
    RAISE EXCEPTION 'cut share lock was not visible';
  END IF;
  UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3;
  RAISE EXCEPTION 'value update committed while the cut lock was visible';
EXCEPTION
  WHEN lock_not_available THEN
    RAISE EXCEPTION '55P03 update';
END
$probe$;
EOF

write_seq_probe() {
  file=$1
  app=$2
  db=$3
  nsp=$4
  rel=$5
  call=$6
  label=$7
  cat > "$ROOT/run/${file}" <<EOF
\\set VERBOSITY verbose
SET lock_timeout = '2s';
DO \$probe\$
DECLARE
  n int := 0;
  i int;
BEGIN
  FOR i IN 1..300 LOOP
    SELECT count(*) INTO n
    FROM pg_catalog.pg_locks l
    JOIN pg_catalog.pg_class c ON c.oid = l.relation
    JOIN pg_catalog.pg_namespace ns ON ns.oid = c.relnamespace
    JOIN pg_catalog.pg_stat_activity a ON a.pid = l.pid
    WHERE a.application_name = '${app}'
      AND a.datname = '${db}'
      AND l.granted AND l.mode = 'ShareRowExclusiveLock'
      AND ns.nspname = '${nsp}' AND c.relname = '${rel}';
    EXIT WHEN n >= 1;
    PERFORM pg_sleep(0.1);
  END LOOP;
  IF n < 1 THEN
    RAISE EXCEPTION 'sequence fence was not visible';
  END IF;
  PERFORM ${call};
  RAISE EXCEPTION '${label} committed while the sequence fence was visible';
EXCEPTION
  WHEN lock_not_available THEN
    RAISE EXCEPTION '55P03 ${label}';
END
\$probe\$;
EOF
}

write_seq_probe q-next-agent.sql oo_a356_cut_agent oo_a356_agent shop cut_seq "nextval('shop.cut_seq')" nextval
write_seq_probe q-next-control.sql oo_a356_cut_control oo_a356_control oo_key_fixture cut_seq "nextval('oo_key_fixture.cut_seq')" nextval
write_seq_probe q-setval-agent.sql oo_a356_cut_agent oo_a356_agent shop cut_seq "setval('shop.cut_seq', 99, true)" setval
write_seq_probe q-setval-control.sql oo_a356_cut_control oo_a356_control oo_key_fixture cut_seq "setval('oo_key_fixture.cut_seq', 99, true)" setval

cat > "$ROOT/run/q-ddl-agent.sql" <<'EOF'
\set ON_ERROR_STOP off
\set VERBOSITY verbose
SET lock_timeout = '2s';
DO $$ BEGIN COMMENT ON VIEW oo_dict.enums IS 'a391-during-cut'; RAISE NOTICE 'step comment ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step comment %', SQLSTATE; END $$;
DO $$ BEGIN CREATE SCHEMA oo_a391_probe; RAISE NOTICE 'step schema ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step schema %', SQLSTATE; END $$;
DO $$ BEGIN CREATE TABLE oo_a391_probe.added (id integer primary key); RAISE NOTICE 'step table ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step table %', SQLSTATE; END $$;
DO $$ BEGIN INSERT INTO oo_a391_probe.added (id) VALUES (7); RAISE NOTICE 'step insert ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step insert %', SQLSTATE; END $$;
DO $$ BEGIN CREATE SEQUENCE oo_a391_probe.seq; RAISE NOTICE 'step sequence ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step sequence %', SQLSTATE; END $$;
DO $$ BEGIN PERFORM nextval('oo_a391_probe.seq'); RAISE NOTICE 'step nextval ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step nextval %', SQLSTATE; END $$;
DO $$ BEGIN CREATE VIEW oo_a391_probe.v AS SELECT 1 AS n; RAISE NOTICE 'step view ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step view %', SQLSTATE; END $$;
EOF
cat > "$ROOT/run/q-ddl-control.sql" <<'EOF'
\set ON_ERROR_STOP off
\set VERBOSITY verbose
SET lock_timeout = '2s';
DO $$ BEGIN COMMENT ON VIEW oo_dict.calendar IS 'a391-during-cut'; RAISE NOTICE 'step comment ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step comment %', SQLSTATE; END $$;
DO $$ BEGIN CREATE SCHEMA oo_a391_probe; RAISE NOTICE 'step schema ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step schema %', SQLSTATE; END $$;
DO $$ BEGIN CREATE TABLE oo_a391_probe.added (id integer primary key); RAISE NOTICE 'step table ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step table %', SQLSTATE; END $$;
DO $$ BEGIN INSERT INTO oo_a391_probe.added (id) VALUES (7); RAISE NOTICE 'step insert ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step insert %', SQLSTATE; END $$;
DO $$ BEGIN CREATE SEQUENCE oo_a391_probe.seq; RAISE NOTICE 'step sequence ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step sequence %', SQLSTATE; END $$;
DO $$ BEGIN PERFORM nextval('oo_a391_probe.seq'); RAISE NOTICE 'step nextval ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step nextval %', SQLSTATE; END $$;
DO $$ BEGIN CREATE VIEW oo_a391_probe.v AS SELECT 1 AS n; RAISE NOTICE 'step view ok'; EXCEPTION WHEN OTHERS THEN RAISE NOTICE 'step view %', SQLSTATE; END $$;
EOF

backup_start=$(date +%s)
set +e
invoke backup "$OPERATOR" > "$LIVE/phases/backup.out" 2> "$LIVE/phases/backup.err" &
backup_pid=$!
set -e
printf '%s\n' "$backup_pid" > "$LIVE/phases/backup.pid"

sql_bg oo_a356_agent q-reject.sql "$LIVE/boundary/reject"
sql_bg oo_a356_agent q-next-agent.sql "$LIVE/boundary/next-agent"
sql_bg oo_a356_control q-next-control.sql "$LIVE/boundary/next-control"
sql_bg oo_a356_agent q-setval-agent.sql "$LIVE/boundary/setval-agent"
sql_bg oo_a356_control q-setval-control.sql "$LIVE/boundary/setval-control"

dump_seen=0
for _try in $(seq 1 180); do
  if ! kill -0 "$backup_pid" 2>/dev/null; then
    break
  fi
  docker exec -u 0 "$PGNAME" cat /var/lib/postgresql/a391-work/dump-flags.txt > "$LIVE/boundary/dump-flags-progress.txt" 2>/dev/null || true
  if grep -q 'oo_a356_' "$LIVE/boundary/dump-flags-progress.txt" 2>/dev/null; then
    dump_seen=1
    break
  fi
  sleep 1
done
printf 'dump_flags_seen=%s\n' "$dump_seen" > "$LIVE/boundary/probe-timing.txt"
if [ "$dump_seen" -ne 1 ]; then
  echo "dump did not start during backup" >&2
  wait "$backup_pid" || true
  exit 1
fi

sql_out oo_a356_agent "$LIVE/boundary/seq-during-agent.txt" "$(seq_state_sql shop.cut_seq agent)"
sql_out oo_a356_control "$LIVE/boundary/seq-during-control.txt" "$(seq_state_sql oo_key_fixture.cut_seq control)"
sql_out oo_a356_agent "$LIVE/boundary/lock-mode-agent.txt" "SELECT l.mode FROM pg_catalog.pg_locks l JOIN pg_catalog.pg_class c ON c.oid = l.relation JOIN pg_catalog.pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_agent' AND c.relname = 'cut_seq' AND l.granted"
sql_bg oo_a356_agent q-ddl-agent.sql "$LIVE/boundary/agent-ddl"
sql_bg oo_a356_control q-ddl-control.sql "$LIVE/boundary/control-ddl"

for dest in "$LIVE/boundary/reject" "$LIVE/boundary/next-agent" "$LIVE/boundary/next-control" "$LIVE/boundary/setval-agent" "$LIVE/boundary/setval-control" "$LIVE/boundary/agent-ddl" "$LIVE/boundary/control-ddl"; do
  if [ -f "${dest}.pid" ]; then
    wait "$(cat "${dest}.pid")" || true
  fi
done

set +e
wait "$backup_pid"
backup_code=$?
set -e
printf '%s\n' "$backup_code" > "$LIVE/phases/backup.exit"
backup_end=$(date +%s)
printf 'backup_seconds=%s\n' "$((backup_end - backup_start))" >> "$LIVE/boundary/probe-timing.txt"
if [ "$backup_code" -ne 0 ]; then
  echo "backup exit ${backup_code}" >&2
  exit 1
fi

sql_out oo_a356_agent "$LIVE/boundary/seq-after-agent.txt" "$(seq_state_sql shop.cut_seq agent)"
sql_out oo_a356_control "$LIVE/boundary/seq-after-control.txt" "$(seq_state_sql oo_key_fixture.cut_seq control)"
sql_out postgres "$LIVE/boundary/cut-sessions-after.txt" "SELECT count(*)::text FROM pg_catalog.pg_stat_activity WHERE application_name IN ('oo_a356_cut_agent','oo_a356_cut_control')"
sql_out oo_a356_agent "$LIVE/boundary/amount-after-update.txt" "UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3 RETURNING amount::text"
phase mark-post-cut 0 mark-post-cut "$OPERATOR"
sql_out postgres "$LIVE/boundary/dbs-before-corrupt.txt" "SELECT datname FROM pg_database WHERE datname IN ('oo_a356_restore_control','oo_a356_restore_agent') ORDER BY 1"
none_if_empty "$LIVE/boundary/dbs-before-corrupt.txt"

docker cp "$PGNAME":/var/lib/postgresql/a391-work/dump-flags.txt "$LIVE/boundary/dump-flags.txt"
docker cp "$PGNAME":/var/lib/postgresql/a391-backup/manifest.json "$LIVE/check-input/manifest.json"
docker cp "$PGNAME":/var/lib/postgresql/a391-backup/manifest.sha256 "$LIVE/check-input/manifest.sha256"
docker cp "$PGNAME":/var/lib/postgresql/a391-backup/catalog.txt "$LIVE/check-input/catalog.txt"
mkdir -p "$LIVE/pristine"
docker cp "$PGNAME":/var/lib/postgresql/a391-backup/oo_a356_agent.dump "$LIVE/pristine/oo_a356_agent.dump"
docker cp "$PGNAME":/var/lib/postgresql/a391-backup/oo_a356_control.dump "$LIVE/pristine/oo_a356_control.dump"
cp "$LIVE/pristine/oo_a356_agent.dump" "$LIVE/check-input/dumps/oo_a356_agent.dump"
cp "$LIVE/pristine/oo_a356_control.dump" "$LIVE/check-input/dumps/oo_a356_control.dump"
docker cp "$PGNAME":/var/lib/postgresql/a391-work/readiness.json "$LIVE/check-input/backup-readiness.json" 2>/dev/null || true

python3 - <<'PY' "$LIVE/check-input/dumps/oo_a356_agent.dump"
import pathlib, sys
path = pathlib.Path(sys.argv[1])
data = bytearray(path.read_bytes())
data[min(128, len(data) - 1)] ^= 0x01
path.write_bytes(data)
PY
docker cp "$LIVE/check-input/dumps/oo_a356_agent.dump" "$PGNAME":/var/lib/postgresql/a391-backup/oo_a356_agent.dump
phase restore-corrupt-dump 4 restore "$OPERATOR"
docker cp "$PGNAME":/var/lib/postgresql/a391-work/readiness.json "$LIVE/corrupt-dump-readiness.json"
sql_out postgres "$LIVE/boundary/dbs-after-corrupt.txt" "SELECT datname FROM pg_database WHERE datname IN ('oo_a356_restore_control','oo_a356_restore_agent') ORDER BY 1"
none_if_empty "$LIVE/boundary/dbs-after-corrupt.txt"
docker cp "$LIVE/pristine/oo_a356_agent.dump" "$PGNAME":/var/lib/postgresql/a391-backup/oo_a356_agent.dump
cp "$LIVE/pristine/oo_a356_agent.dump" "$LIVE/check-input/dumps/oo_a356_agent.dump"

restore_start=$(date +%s)
phase restore 0 restore "$OPERATOR"
restore_end=$(date +%s)
printf 'restore_seconds=%s\n' "$((restore_end - restore_start))" >> "$LIVE/boundary/probe-timing.txt"
docker cp "$PGNAME":/var/lib/postgresql/a391-work/readiness.json "$LIVE/restore-readiness.json"
phase restore-existing 4 restore "$OPERATOR"

sql_out oo_a356_restore_agent "$LIVE/boundary/restore-seq-agent.txt" "SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM shop.cut_seq"
sql_out oo_a356_restore_control "$LIVE/boundary/restore-seq-control.txt" "SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END || '|' || log_cnt::text FROM oo_key_fixture.cut_seq"
sql_out oo_a356_restore_agent "$LIVE/boundary/restore-amount.txt" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"
sql_out oo_a356_restore_agent "$LIVE/boundary/restore-views-agent.txt" "SELECT n.nspname || '.' || c.relname || '|' || coalesce(d.description, '') FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace LEFT JOIN pg_catalog.pg_description d ON d.objoid = c.oid AND d.classoid = 'pg_catalog.pg_class'::regclass AND d.objsubid = 0 WHERE c.relkind = 'v' AND n.nspname NOT IN ('pg_catalog','information_schema') ORDER BY 1"
sql_out oo_a356_restore_control "$LIVE/boundary/restore-views-control.txt" "SELECT n.nspname || '.' || c.relname || '|' || coalesce(d.description, '') FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace LEFT JOIN pg_catalog.pg_description d ON d.objoid = c.oid AND d.classoid = 'pg_catalog.pg_class'::regclass AND d.objsubid = 0 WHERE c.relkind = 'v' AND n.nspname NOT IN ('pg_catalog','information_schema') ORDER BY 1"
sql_out oo_a356_restore_agent "$LIVE/boundary/restore-probe-agent.txt" "SELECT count(*)::text FROM pg_catalog.pg_namespace WHERE nspname = 'oo_a391_probe'"
sql_out oo_a356_restore_control "$LIVE/boundary/restore-probe-control.txt" "SELECT count(*)::text FROM pg_catalog.pg_namespace WHERE nspname = 'oo_a391_probe'"
sql_out oo_a356_agent "$LIVE/boundary/source-probe.txt" "SELECT count(*)::text FROM pg_catalog.pg_namespace WHERE nspname = 'oo_a391_probe'"
sql_out oo_a356_agent "$LIVE/boundary/source-comment.txt" "SELECT coalesce(d.description, '') FROM pg_catalog.pg_description d JOIN pg_catalog.pg_class c ON c.oid = d.objoid WHERE c.oid = 'oo_dict.enums'::regclass AND d.objsubid = 0"
sql_out oo_a356_restore_agent "$LIVE/boundary/restore-party-comment.txt" "SELECT coalesce(d.description, '') FROM pg_catalog.pg_description d WHERE d.objoid = 'shop.party'::regclass AND d.objsubid = 0"
sql_out oo_a356_agent "$LIVE/boundary/source-nextval.txt" "SELECT nextval('shop.cut_seq')::text"
sql_out oo_a356_agent "$LIVE/boundary/source-after-nextval.txt" "SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END FROM shop.cut_seq"
sql_out oo_a356_restore_agent "$LIVE/boundary/counts.txt" "SELECT (SELECT count(*) FROM shop.party)::text || '|' || (SELECT count(*) FROM shop.account)::text || '|' || (SELECT count(*) FROM shop.entry)::text"
sql_out oo_a356_restore_agent "$LIVE/boundary/post-cut-party.txt" "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'"
sql_out oo_a356_agent "$LIVE/boundary/source-post-cut-party.txt" "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'"

cat > "$ROOT/run/list-setval.sh" <<EOF
#!/bin/bash
set -euo pipefail
${REAL_TOOLDIR}/pg_restore -f - /var/lib/postgresql/a391-backup/oo_a356_agent.dump | grep -E "setval\\(|CREATE VIEW|a391-during-cut|oo_a391_probe" || true
echo '---control---'
${REAL_TOOLDIR}/pg_restore -f - /var/lib/postgresql/a391-backup/oo_a356_control.dump | grep -E "setval\\(|CREATE VIEW|a391-during-cut|oo_a391_probe" || true
EOF
docker cp "$ROOT/run/list-setval.sh" "$PGNAME":/var/lib/postgresql/a391-work/list-setval.sh
docker exec -u 0 "$PGNAME" chmod 755 /var/lib/postgresql/a391-work/list-setval.sh
docker exec -u 0 "$PGNAME" /var/lib/postgresql/a391-work/list-setval.sh > "$LIVE/boundary/dump-setval.txt"
