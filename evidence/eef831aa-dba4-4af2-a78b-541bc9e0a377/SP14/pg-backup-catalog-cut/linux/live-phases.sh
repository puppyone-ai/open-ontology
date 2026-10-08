#!/bin/bash
# Load the 100k fixture, hold the cut, and probe catalog changes during that window.
# Sourced by run-live.sh. Does not start PostgreSQL or a Rust builder.

if [ -z "${PGNAME:-}" ] || [ -z "${BIN:-}" ] || [ -z "${OPERATOR:-}" ] || [ -z "${REAL_TOOLDIR:-}" ]; then
  echo "live-phases.sh is sourced by run-live.sh" >&2
  exit 2
fi

phase() {
  name=$1
  expect=$2
  shift 2
  set +e
  invoke "$@" > "$EVIDENCE/live/phases/${name}.out" 2> "$EVIDENCE/live/phases/${name}.err"
  code=$?
  set -e
  printf '%s\n' "$code" > "$EVIDENCE/live/phases/${name}.exit"
  echo "phase ${name} exit ${code}"
  if [ "$code" -ne "$expect" ]; then
    docker cp "$PGNAME":/var/lib/postgresql/a377-work/readiness.json "$EVIDENCE/live/phases/${name}-readiness.json" 2>/dev/null || true
    echo "phase ${name} expected ${expect}" >&2
    exit 1
  fi
}

sql_out() {
  db=$1
  dest=$2
  sql=$3
  printf '%s\n' "$sql" > "$ROOT/run/query.sql"
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a377-work/psql-exec.sh "$db" > "$dest"
}

sql_status() {
  db=$1
  dest_out=$2
  dest_err=$3
  dest_exit=$4
  sqlfile=${5:-query.sql}
  set +e
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a377-work/psql-exec.sh "$db" "$sqlfile" > "$dest_out" 2> "$dest_err"
  code=$?
  set -e
  printf '%s\n' "$code" > "$dest_exit"
  echo "sql ${dest_exit} exit ${code}"
}

trim_copy() {
  src=$1
  dest=$2
  tr -d '[:space:]' < "$src" > "$dest"
  printf '\n' >> "$dest"
}

none_if_empty() {
  if [ ! -s "$1" ] || [ -z "$(tr -d '[:space:]' < "$1")" ]; then
    printf '%s\n' none > "$1"
  fi
}

list_restore_dbs() {
  dest=$1
  sql_out postgres "$dest" "SELECT datname FROM pg_database WHERE datname IN ('oo_a356_restore_control','oo_a356_restore_agent') ORDER BY 1"
  none_if_empty "$dest"
}

one_line() {
  if [ -s "$1" ]; then
    tr -d '[:space:]' < "$1"
  else
    printf none
  fi
}

mkdir -p "$EVIDENCE/live/phases" "$EVIDENCE/live/boundary" "$ROOT/saved/dumps" "$ROOT/saved/gold" "$ROOT/saved/probes" "$ROOT/saved/reports" "$ROOT/check"
docker cp "$EVIDENCE/linux/psql-exec.sh" "$PGNAME":/var/lib/postgresql/a377-work/psql-exec.sh
docker exec -u 0 "$PGNAME" chmod 755 /var/lib/postgresql/a377-work/psql-exec.sh

phase load-fixture 0 load-fixture "$OPERATOR"

backup_start=$(date +%s)
set +e
invoke backup "$OPERATOR" > "$EVIDENCE/live/phases/backup.out" 2> "$EVIDENCE/live/phases/backup.err" &
backup_pid=$!
set -e
printf '%s\n' "$backup_pid" > "$EVIDENCE/live/phases/backup.pid"
cleanup_backup() {
  if [ -z "${reject_done:-}" ] && [ -f "$ROOT/run/reject.pid" ]; then
    kill "$(cat "$ROOT/run/reject.pid")" 2>/dev/null || true
  fi
  if [ -n "${backup_pid:-}" ] && kill -0 "$backup_pid" 2>/dev/null; then
    kill "$backup_pid" 2>/dev/null || true
    wait "$backup_pid" 2>/dev/null || true
  fi
}
trap cleanup_backup EXIT

# The value update shares the cut session's lifetime. It waits until that
# session's SHARE lock is visible, then blocks on the row change. A timeout
# re-raises 55P03 and leaves the row unchanged. A path that would commit
# raises and rolls back, so a gap cannot record a new amount as the cut.
cat > "$ROOT/run/q-reject.sql" <<'EOF'
\set VERBOSITY verbose
SET lock_timeout = '1s';
DO $probe$
DECLARE
  n int := 0;
  i int;
BEGIN
  FOR i IN 1..200 LOOP
    SELECT count(*) INTO n
    FROM pg_locks l
    JOIN pg_class c ON c.oid = l.relation
    JOIN pg_namespace ns ON ns.oid = c.relnamespace
    JOIN pg_stat_activity a ON a.pid = l.pid
    WHERE a.application_name = 'oo_a356_cut_agent'
      AND a.datname = 'oo_a356_agent'
      AND l.granted
      AND l.mode = 'ShareLock'
      AND ns.nspname = 'shop'
      AND c.relname = 'entry';
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
    RAISE;
END
$probe$;
EOF
docker exec -u 0 "$PGNAME" sh -c 'grep -q oo_a356_cut_agent /run-in/q-reject.sql && grep -q "amount = 1.0001" /run-in/q-reject.sql'
docker exec -u 0 "$PGNAME" /var/lib/postgresql/a377-work/psql-exec.sh oo_a356_agent q-reject.sql > "$ROOT/saved/probes/rejected.out" 2> "$ROOT/saved/probes/rejected.err" &
printf '%s\n' "$!" > "$ROOT/run/reject.pid"

seen=0
agent_locks=""
control_locks=""
for _try in $(seq 1 150); do
  if ! kill -0 "$backup_pid" 2>/dev/null; then
    break
  fi
  sql_out oo_a356_agent "$ROOT/saved/probes/lock-agent.raw" "SELECT count(*)::text FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_agent' AND l.granted AND l.mode = 'ShareLock' AND n.nspname = 'shop' AND c.relname = 'entry'"
  sql_out oo_a356_control "$ROOT/saved/probes/lock-control.raw" "SELECT count(*)::text FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_control' AND l.granted AND l.mode = 'ShareLock' AND n.nspname = 'oo_key_fixture' AND c.relname = 'cut_marker'"
  agent_locks=$(tr -d '[:space:]' < "$ROOT/saved/probes/lock-agent.raw")
  control_locks=$(tr -d '[:space:]' < "$ROOT/saved/probes/lock-control.raw")
  if [ "$agent_locks" = "1" ] && [ "$control_locks" = "1" ]; then
    seen=1
    break
  fi
  sleep 0.2
done
printf 'seen=%s agent=%s control=%s\n' "$seen" "${agent_locks:-}" "${control_locks:-}" > "$EVIDENCE/live/phases/locks-seen.txt"
if [ "$seen" -ne 1 ]; then
  echo "writer locks were not visible during backup" >&2
  wait "$backup_pid" || true
  exit 1
fi

cat > "$ROOT/run/q-next-agent.sql" <<'EOF'
\set VERBOSITY verbose
SET lock_timeout = '1s';
DO $probe$
DECLARE
  n int := 0;
  i int;
BEGIN
  FOR i IN 1..200 LOOP
    SELECT count(*) INTO n
    FROM pg_locks l
    JOIN pg_class c ON c.oid = l.relation
    JOIN pg_namespace ns ON ns.oid = c.relnamespace
    JOIN pg_stat_activity a ON a.pid = l.pid
    WHERE a.application_name = 'oo_a356_cut_agent'
      AND a.datname = 'oo_a356_agent'
      AND l.granted
      AND l.mode = 'ShareRowExclusiveLock'
      AND ns.nspname = 'shop'
      AND c.relname = 'cut_seq';
    EXIT WHEN n >= 1;
    PERFORM pg_sleep(0.1);
  END LOOP;
  IF n < 1 THEN
    RAISE EXCEPTION 'sequence fence was not visible';
  END IF;
  PERFORM nextval('shop.cut_seq');
  RAISE EXCEPTION 'nextval committed while the sequence fence was visible';
EXCEPTION
  WHEN lock_not_available THEN
    RAISE EXCEPTION '55P03 nextval %', SQLERRM USING ERRCODE = 'lock_not_available';
END
$probe$;
EOF
cat > "$ROOT/run/q-next-control.sql" <<'EOF'
\set VERBOSITY verbose
SET lock_timeout = '1s';
DO $probe$
DECLARE
  n int := 0;
  i int;
BEGIN
  FOR i IN 1..200 LOOP
    SELECT count(*) INTO n
    FROM pg_locks l
    JOIN pg_class c ON c.oid = l.relation
    JOIN pg_namespace ns ON ns.oid = c.relnamespace
    JOIN pg_stat_activity a ON a.pid = l.pid
    WHERE a.application_name = 'oo_a356_cut_control'
      AND a.datname = 'oo_a356_control'
      AND l.granted
      AND l.mode = 'ShareRowExclusiveLock'
      AND ns.nspname = 'oo_key_fixture'
      AND c.relname = 'cut_seq';
    EXIT WHEN n >= 1;
    PERFORM pg_sleep(0.1);
  END LOOP;
  IF n < 1 THEN
    RAISE EXCEPTION 'sequence fence was not visible';
  END IF;
  PERFORM nextval('oo_key_fixture.cut_seq');
  RAISE EXCEPTION 'nextval committed while the sequence fence was visible';
EXCEPTION
  WHEN lock_not_available THEN
    RAISE EXCEPTION '55P03 nextval %', SQLERRM USING ERRCODE = 'lock_not_available';
END
$probe$;
EOF
cat > "$ROOT/run/q-setval-agent.sql" <<'EOF'
\set VERBOSITY verbose
SET lock_timeout = '1s';
DO $probe$
DECLARE
  n int := 0;
  i int;
BEGIN
  FOR i IN 1..200 LOOP
    SELECT count(*) INTO n
    FROM pg_locks l
    JOIN pg_class c ON c.oid = l.relation
    JOIN pg_namespace ns ON ns.oid = c.relnamespace
    JOIN pg_stat_activity a ON a.pid = l.pid
    WHERE a.application_name = 'oo_a356_cut_agent'
      AND a.datname = 'oo_a356_agent'
      AND l.granted
      AND l.mode = 'ShareRowExclusiveLock'
      AND ns.nspname = 'shop'
      AND c.relname = 'cut_seq';
    EXIT WHEN n >= 1;
    PERFORM pg_sleep(0.1);
  END LOOP;
  IF n < 1 THEN
    RAISE EXCEPTION 'sequence fence was not visible';
  END IF;
  PERFORM setval('shop.cut_seq', 99, true);
  RAISE EXCEPTION 'setval committed while the sequence fence was visible';
EXCEPTION
  WHEN lock_not_available THEN
    RAISE EXCEPTION '55P03 setval %', SQLERRM USING ERRCODE = 'lock_not_available';
END
$probe$;
EOF
cat > "$ROOT/run/q-setval-control.sql" <<'EOF'
\set VERBOSITY verbose
SET lock_timeout = '1s';
DO $probe$
DECLARE
  n int := 0;
  i int;
BEGIN
  FOR i IN 1..200 LOOP
    SELECT count(*) INTO n
    FROM pg_locks l
    JOIN pg_class c ON c.oid = l.relation
    JOIN pg_namespace ns ON ns.oid = c.relnamespace
    JOIN pg_stat_activity a ON a.pid = l.pid
    WHERE a.application_name = 'oo_a356_cut_control'
      AND a.datname = 'oo_a356_control'
      AND l.granted
      AND l.mode = 'ShareRowExclusiveLock'
      AND ns.nspname = 'oo_key_fixture'
      AND c.relname = 'cut_seq';
    EXIT WHEN n >= 1;
    PERFORM pg_sleep(0.1);
  END LOOP;
  IF n < 1 THEN
    RAISE EXCEPTION 'sequence fence was not visible';
  END IF;
  PERFORM setval('oo_key_fixture.cut_seq', 99, true);
  RAISE EXCEPTION 'setval committed while the sequence fence was visible';
EXCEPTION
  WHEN lock_not_available THEN
    RAISE EXCEPTION '55P03 setval %', SQLERRM USING ERRCODE = 'lock_not_available';
END
$probe$;
EOF
cat > "$ROOT/run/q-agent-ddl.sql" <<'EOF'
\set ON_ERROR_STOP off
\set VERBOSITY verbose
SET lock_timeout = '2s';
DO $$
BEGIN
  COMMENT ON VIEW oo_dict.enums IS 'a377-during-cut';
  RAISE NOTICE 'step comment ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step comment %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE SCHEMA oo_a377_probe;
  RAISE NOTICE 'step schema ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step schema %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE TABLE oo_a377_probe.added (id integer primary key);
  RAISE NOTICE 'step table ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step table %', SQLSTATE;
END $$;
DO $$
BEGIN
  INSERT INTO oo_a377_probe.added (id) VALUES (7);
  RAISE NOTICE 'step insert ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step insert %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE SEQUENCE oo_a377_probe.seq;
  RAISE NOTICE 'step sequence ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step sequence %', SQLSTATE;
END $$;
DO $$
BEGIN
  PERFORM nextval('oo_a377_probe.seq');
  RAISE NOTICE 'step nextval ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step nextval %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE VIEW oo_a377_probe.v AS SELECT 7 AS n;
  RAISE NOTICE 'step view ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step view %', SQLSTATE;
END $$;
EOF
cp "$ROOT/run/q-agent-ddl.sql" "$ROOT/run/q-control-ddl.sql"
perl -0pi -e 's/oo_dict\.enums/oo_dict.calendar/g' "$ROOT/run/q-control-ddl.sql"

start_sql() {
  name=$1
  db=$2
  sqlfile=$3
  dest_out=$4
  dest_err=$5
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a377-work/psql-exec.sh "$db" "$sqlfile" > "$dest_out" 2> "$dest_err" &
  printf '%s\n' "$!" > "$ROOT/run/${name}.pid"
}

finish_sql() {
  name=$1
  dest_exit=$2
  pid=$(cat "$ROOT/run/${name}.pid")
  set +e
  wait "$pid"
  code=$?
  set -e
  printf '%s\n' "$code" > "$dest_exit"
  echo "sql ${name} exit ${code}"
}

probe_start=$(date +%s)
start_sql next-agent oo_a356_agent q-next-agent.sql "$EVIDENCE/live/boundary/next-agent.out" "$EVIDENCE/live/boundary/next-agent.err"
start_sql next-control oo_a356_control q-next-control.sql "$EVIDENCE/live/boundary/next-control.out" "$EVIDENCE/live/boundary/next-control.err"
start_sql setval-agent oo_a356_agent q-setval-agent.sql "$EVIDENCE/live/boundary/setval-agent.out" "$EVIDENCE/live/boundary/setval-agent.err"
start_sql setval-control oo_a356_control q-setval-control.sql "$EVIDENCE/live/boundary/setval-control.out" "$EVIDENCE/live/boundary/setval-control.err"
start_sql agent-ddl oo_a356_agent q-agent-ddl.sql "$EVIDENCE/live/boundary/agent-ddl.out" "$EVIDENCE/live/boundary/agent-ddl.err"
start_sql control-ddl oo_a356_control q-control-ddl.sql "$EVIDENCE/live/boundary/control-ddl.out" "$EVIDENCE/live/boundary/control-ddl.err"
sql_out oo_a356_agent "$ROOT/saved/probes/sequence-agent-before.raw" "SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END FROM shop.cut_seq"
sql_out oo_a356_control "$ROOT/saved/probes/sequence-control-before.raw" "SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END FROM oo_key_fixture.cut_seq"
trim_copy "$ROOT/saved/probes/sequence-agent-before.raw" "$ROOT/saved/probes/sequence-agent-before.txt"
trim_copy "$ROOT/saved/probes/sequence-control-before.raw" "$ROOT/saved/probes/sequence-control-before.txt"
agent_seq=$(tr -d '[:space:]' < "$ROOT/saved/probes/sequence-agent-before.txt")
control_seq=$(tr -d '[:space:]' < "$ROOT/saved/probes/sequence-control-before.txt")
if [ "$agent_seq" = "41|f" ] && [ "$control_seq" = "41|f" ]; then
  printf '%s\n' '41|f' > "$ROOT/saved/probes/sequence-before.txt"
else
  printf '%s\n' "${agent_seq}|${control_seq}" > "$ROOT/saved/probes/sequence-before.txt"
fi
sql_out oo_a356_agent "$EVIDENCE/live/boundary/agent-lock-during.out" "SELECT count(*)::text FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_agent' AND l.granted AND l.mode = 'ShareLock' AND n.nspname = 'shop' AND c.relname = 'entry'"
sql_out oo_a356_control "$EVIDENCE/live/boundary/control-lock-during.out" "SELECT count(*)::text FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_control' AND l.granted AND l.mode = 'ShareLock' AND n.nspname = 'oo_key_fixture' AND c.relname = 'cut_marker'"
during_agent=$(one_line "$EVIDENCE/live/boundary/agent-lock-during.out")
during_control=$(one_line "$EVIDENCE/live/boundary/control-lock-during.out")
if [ "$during_agent" != "1" ] || [ "$during_control" != "1" ]; then
  echo "writer locks were not held while the value update was in flight" >&2
  finish_sql reject "$ROOT/saved/probes/rejected.exit" || true
  exit 1
fi
finish_sql next-agent "$EVIDENCE/live/boundary/next-agent.exit"
finish_sql next-control "$EVIDENCE/live/boundary/next-control.exit"
finish_sql setval-agent "$EVIDENCE/live/boundary/setval-agent.exit"
finish_sql setval-control "$EVIDENCE/live/boundary/setval-control.exit"
finish_sql agent-ddl "$EVIDENCE/live/boundary/agent-ddl.exit"
finish_sql control-ddl "$EVIDENCE/live/boundary/control-ddl.exit"
sql_out oo_a356_agent "$EVIDENCE/live/boundary/agent-lock-after.out" "SELECT count(*)::text FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_agent' AND l.granted AND l.mode = 'ShareLock' AND n.nspname = 'shop' AND c.relname = 'entry'"
sql_out oo_a356_control "$EVIDENCE/live/boundary/control-lock-after.out" "SELECT count(*)::text FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_control' AND l.granted AND l.mode = 'ShareLock' AND n.nspname = 'oo_key_fixture' AND c.relname = 'cut_marker'"
finish_sql reject "$ROOT/saved/probes/rejected.exit"
reject_done=1
probe_done=$(date +%s)
printf 'probe_start=%s\nprobe_done=%s\n' "$probe_start" "$probe_done" > "$EVIDENCE/live/boundary/probe-timing.txt"
if ! grep -q '55P03' "$ROOT/saved/probes/rejected.err"; then
  echo "value update was not rejected while the cut was in progress" >&2
  exit 1
fi
sql_out oo_a356_agent "$ROOT/saved/probes/amount-during-lock.raw" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"
trim_copy "$ROOT/saved/probes/amount-during-lock.raw" "$ROOT/saved/probes/amount-during-lock.txt"
sql_out oo_a356_control "$ROOT/saved/probes/marker-before.raw" "SELECT source_cut || '|' || entry_count::text FROM oo_key_fixture.cut_marker"
sql_out oo_a356_agent "$ROOT/saved/probes/count-before.raw" "SELECT count(*)::text FROM shop.entry"
trim_copy "$ROOT/saved/probes/marker-before.raw" "$ROOT/saved/probes/marker-before.txt"
trim_copy "$ROOT/saved/probes/count-before.raw" "$ROOT/saved/probes/count-before.txt"

cat "$EVIDENCE/live/boundary/next-agent.err" "$EVIDENCE/live/boundary/next-control.err" \
  "$EVIDENCE/live/boundary/setval-agent.err" "$EVIDENCE/live/boundary/setval-control.err" \
  > "$ROOT/saved/probes/preexisting-nextval.err"
sequence_blocked=1
for probe_name in next-agent next-control setval-agent setval-control; do
  probe_code=$(cat "$EVIDENCE/live/boundary/${probe_name}.exit")
  if [ "$probe_code" = "0" ] || ! grep -q '55P03' "$EVIDENCE/live/boundary/${probe_name}.err"; then
    sequence_blocked=0
    echo "sequence probe ${probe_name} exit ${probe_code} did not return 55P03" >&2
  fi
done
if [ "$sequence_blocked" -ne 1 ]; then
  printf '%s\n' 0 > "$ROOT/saved/probes/preexisting-nextval.exit"
  wait "$backup_pid" || true
  backup_pid=""
  echo "sequence fence did not reject nextval and setval" >&2
  exit 1
fi
printf '%s\n' 1 > "$ROOT/saved/probes/preexisting-nextval.exit"

set +e
wait "$backup_pid"
backup_code=$?
set -e
backup_pid=""
backup_end=$(date +%s)
printf '%s\n' "$backup_code" > "$EVIDENCE/live/phases/backup.exit"
printf 'backup_outer_start=%s\nbackup_outer_end=%s\n' "$backup_start" "$backup_end" > "$EVIDENCE/live/timing.txt"
cat "$EVIDENCE/live/boundary/probe-timing.txt" >> "$EVIDENCE/live/timing.txt"
echo "phase backup exit ${backup_code}"
if [ "$backup_code" -ne 0 ]; then
  echo "backup failed" >&2
  exit 1
fi

sql_out postgres "$ROOT/saved/probes/cut-sessions-after.raw" "SELECT count(*)::text FROM pg_stat_activity WHERE application_name IN ('oo_a356_cut_control','oo_a356_cut_agent')"
trim_copy "$ROOT/saved/probes/cut-sessions-after.raw" "$ROOT/saved/probes/cut-sessions-after.txt"
sql_out oo_a356_agent "$ROOT/saved/probes/sequence-agent-after.raw" "SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END FROM shop.cut_seq"
sql_out oo_a356_control "$ROOT/saved/probes/sequence-control-after.raw" "SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END FROM oo_key_fixture.cut_seq"
trim_copy "$ROOT/saved/probes/sequence-agent-after.raw" "$ROOT/saved/probes/sequence-agent-after.txt"
trim_copy "$ROOT/saved/probes/sequence-control-after.raw" "$ROOT/saved/probes/sequence-control-after.txt"
agent_after=$(tr -d '[:space:]' < "$ROOT/saved/probes/sequence-agent-after.txt")
control_after=$(tr -d '[:space:]' < "$ROOT/saved/probes/sequence-control-after.txt")
if [ "$agent_after" = "41|f" ] && [ "$control_after" = "41|f" ]; then
  printf '%s\n' '41|f' > "$ROOT/saved/probes/sequence-after.txt"
else
  printf '%s\n' "$agent_after" > "$ROOT/saved/probes/sequence-after.txt"
fi

cat > "$ROOT/run/query.sql" <<'EOF'
SET lock_timeout = '5s';
UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3;
EOF
sql_status oo_a356_agent "$ROOT/saved/probes/committed.out" "$ROOT/saved/probes/committed.err" "$ROOT/saved/probes/committed.exit"
sql_out oo_a356_control "$ROOT/saved/probes/marker-after.raw" "SELECT source_cut || '|' || entry_count::text FROM oo_key_fixture.cut_marker"
sql_out oo_a356_agent "$ROOT/saved/probes/count-after.raw" "SELECT count(*)::text FROM shop.entry"
sql_out oo_a356_agent "$ROOT/saved/probes/source-amount.raw" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"
trim_copy "$ROOT/saved/probes/marker-after.raw" "$ROOT/saved/probes/marker-after.txt"
trim_copy "$ROOT/saved/probes/count-after.raw" "$ROOT/saved/probes/count-after.txt"
trim_copy "$ROOT/saved/probes/source-amount.raw" "$ROOT/saved/probes/source-amount.txt"

phase mark-post-cut 0 mark-post-cut "$OPERATOR"

docker cp "$PGNAME":/var/lib/postgresql/a377-backup/manifest.json "$ROOT/saved/manifest.json"
docker cp "$PGNAME":/var/lib/postgresql/a377-backup/manifest.sha256 "$ROOT/saved/manifest.sha256"
docker cp "$PGNAME":/var/lib/postgresql/a377-backup/catalog.txt "$ROOT/saved/catalog.txt"
docker cp "$PGNAME":/var/lib/postgresql/a377-backup/oo_a356_agent.dump "$ROOT/saved/dumps/oo_a356_agent.dump"
docker cp "$PGNAME":/var/lib/postgresql/a377-backup/oo_a356_control.dump "$ROOT/saved/dumps/oo_a356_control.dump"
docker cp "$PGNAME":/var/lib/postgresql/a377-backup/gold/. "$ROOT/saved/gold/"
docker cp "$PGNAME":/var/lib/postgresql/a377-work/dump-flags.txt "$ROOT/saved/probes/dump-flags.txt"
docker exec -u 0 "$PGNAME" sh -c 'rm -rf /var/lib/postgresql/a377-backup-good && mkdir -p /var/lib/postgresql/a377-backup-good && cp -a /var/lib/postgresql/a377-backup/. /var/lib/postgresql/a377-backup-good/'

docker exec -u 0 "$PGNAME" "$REAL_TOOLDIR/pg_restore" -f - /var/lib/postgresql/a377-backup-good/oo_a356_agent.dump > "$ROOT/saved/agent-restore.sql"
docker exec -u 0 "$PGNAME" "$REAL_TOOLDIR/pg_restore" -f - /var/lib/postgresql/a377-backup-good/oo_a356_control.dump > "$ROOT/saved/control-restore.sql"
if grep -a -q 'oo_a377_probe' "$ROOT/saved/agent-restore.sql" "$ROOT/saved/control-restore.sql" \
  || grep -a -q 'a377-during-cut' "$ROOT/saved/agent-restore.sql" "$ROOT/saved/control-restore.sql"; then
  printf '%s\n' no > "$ROOT/saved/probes/postcut-absent.txt"
else
  printf '%s\n' yes > "$ROOT/saved/probes/postcut-absent.txt"
fi
if grep -a -q '1234567890.1234' "$ROOT/saved/agent-restore.sql"; then
  printf '%s\n' '1234567890.1234' > "$ROOT/saved/probes/dump-amount.txt"
else
  printf '%s\n' missing > "$ROOT/saved/probes/dump-amount.txt"
fi
grep -n 'cut_mode\|"held"' "$ROOT/saved/manifest.json" > "$EVIDENCE/live/boundary/manifest-cut.txt" || true

list_restore_dbs "$ROOT/saved/probes/dbs-before-corrupt.txt"
docker exec -u 0 "$PGNAME" sh -c 'chmod u+w /var/lib/postgresql/a377-backup/oo_a356_agent.dump && dd if=/dev/zero of=/var/lib/postgresql/a377-backup/oo_a356_agent.dump bs=1 count=1 conv=notrunc'
phase restore-corrupt-dump 4 restore "$OPERATOR"
docker cp "$PGNAME":/var/lib/postgresql/a377-work/readiness.json "$EVIDENCE/live/corrupt-dump-readiness.json"
cp "$EVIDENCE/live/corrupt-dump-readiness.json" "$ROOT/saved/reports/corrupt-dump-readiness.json"
list_restore_dbs "$ROOT/saved/probes/dbs-after-corrupt.txt"

docker exec -u 0 "$PGNAME" sh -c 'rm -rf /var/lib/postgresql/a377-backup && mkdir -p /var/lib/postgresql/a377-backup && cp -a /var/lib/postgresql/a377-backup-good/. /var/lib/postgresql/a377-backup/'
if grep -qx '    "held": true,' "$ROOT/saved/manifest.json"; then
  restore_expect=0
else
  restore_expect=4
fi
printf '%s\n' "$restore_expect" > "$EVIDENCE/live/boundary/restore-expect.txt"
restore_start=$(date +%s)
phase restore "$restore_expect" restore "$OPERATOR"
restore_end=$(date +%s)
printf 'restore_outer_start=%s\nrestore_outer_end=%s\n' "$restore_start" "$restore_end" >> "$EVIDENCE/live/timing.txt"
docker cp "$PGNAME":/var/lib/postgresql/a377-work/readiness.json "$EVIDENCE/live/restore-readiness.json"
cp "$EVIDENCE/live/restore-readiness.json" "$ROOT/saved/reports/restore-readiness.json"
if [ "$restore_expect" = "0" ]; then
  sql_out oo_a356_restore_agent "$ROOT/saved/probes/restore-amount.raw" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"
  trim_copy "$ROOT/saved/probes/restore-amount.raw" "$ROOT/saved/probes/restore-amount.txt"
  sql_out oo_a356_restore_agent "$ROOT/saved/probes/restore-sequence-agent.raw" "SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END FROM shop.cut_seq"
  sql_out oo_a356_restore_control "$ROOT/saved/probes/restore-sequence-control.raw" "SELECT last_value::text || '|' || CASE WHEN is_called THEN 't' ELSE 'f' END FROM oo_key_fixture.cut_seq"
  restore_agent_seq=$(tr -d '[:space:]' < "$ROOT/saved/probes/restore-sequence-agent.raw")
  restore_control_seq=$(tr -d '[:space:]' < "$ROOT/saved/probes/restore-sequence-control.raw")
  if [ "$restore_agent_seq" = "41|f" ] && [ "$restore_control_seq" = "41|f" ]; then
    printf '%s\n' '41|f' > "$ROOT/saved/probes/restore-sequence.txt"
  else
    printf '%s\n' "${restore_agent_seq}|${restore_control_seq}" > "$ROOT/saved/probes/restore-sequence.txt"
  fi
  sql_out oo_a356_restore_agent "$ROOT/saved/probes/restore-view-agent.raw" "SELECT CASE WHEN c.relkind = 'v' AND COALESCE(d.description, '') <> 'a377-during-cut' THEN 'yes' ELSE 'no' END FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace LEFT JOIN pg_catalog.pg_description d ON d.objoid = c.oid AND d.classoid = 'pg_catalog.pg_class'::pg_catalog.regclass AND d.objsubid = 0 WHERE n.nspname = 'oo_dict' AND c.relname = 'enums'"
  sql_out oo_a356_restore_control "$ROOT/saved/probes/restore-view-control.raw" "SELECT CASE WHEN c.relkind = 'v' AND COALESCE(d.description, '') <> 'a377-during-cut' THEN 'yes' ELSE 'no' END FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace LEFT JOIN pg_catalog.pg_description d ON d.objoid = c.oid AND d.classoid = 'pg_catalog.pg_class'::pg_catalog.regclass AND d.objsubid = 0 WHERE n.nspname = 'oo_dict' AND c.relname = 'calendar'"
  sql_out oo_a356_restore_agent "$ROOT/saved/probes/restore-probe-schema.raw" "SELECT count(*)::text FROM pg_catalog.pg_namespace WHERE nspname = 'oo_a377_probe'"
  view_agent=$(tr -d '[:space:]' < "$ROOT/saved/probes/restore-view-agent.raw")
  view_control=$(tr -d '[:space:]' < "$ROOT/saved/probes/restore-view-control.raw")
  probe_schema=$(tr -d '[:space:]' < "$ROOT/saved/probes/restore-probe-schema.raw")
  if [ "$view_agent" = "yes" ] && [ "$view_control" = "yes" ] && [ "$probe_schema" = "0" ]; then
    printf '%s\n' yes > "$ROOT/saved/probes/restore-views.txt"
  else
    printf '%s\n' "agent=${view_agent} control=${view_control} probe=${probe_schema}" > "$ROOT/saved/probes/restore-views.txt"
  fi
  phase restore-existing 4 restore "$OPERATOR"
else
  phase restore-again 4 restore "$OPERATOR"
fi

rm -rf "$ROOT/check"
mkdir -p "$ROOT/check/dumps" "$ROOT/check/gold" "$ROOT/check/probes" "$ROOT/check/reports"
cp "$ROOT/saved/manifest.json" "$ROOT/saved/manifest.sha256" "$ROOT/saved/catalog.txt" "$ROOT/check/"
cp "$ROOT/saved/dumps/"* "$ROOT/check/dumps/"
cp "$ROOT/saved/gold/"* "$ROOT/check/gold/"
cp "$ROOT/saved/probes/rejected.err" "$ROOT/saved/probes/rejected.exit" "$ROOT/saved/probes/committed.exit" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/marker-before.txt" "$ROOT/saved/probes/marker-after.txt" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/count-before.txt" "$ROOT/saved/probes/count-after.txt" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/amount-during-lock.txt" "$ROOT/saved/probes/source-amount.txt" "$ROOT/saved/probes/dump-amount.txt" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/postcut-absent.txt" "$ROOT/saved/probes/dump-flags.txt" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/dbs-before-corrupt.txt" "$ROOT/saved/probes/dbs-after-corrupt.txt" "$ROOT/saved/probes/cut-sessions-after.txt" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/sequence-before.txt" "$ROOT/saved/probes/sequence-after.txt" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/preexisting-nextval.err" "$ROOT/saved/probes/preexisting-nextval.exit" "$ROOT/check/probes/"
if [ -f "$ROOT/saved/probes/restore-amount.txt" ]; then
  cp "$ROOT/saved/probes/restore-amount.txt" "$ROOT/check/probes/"
fi
if [ -f "$ROOT/saved/probes/restore-sequence.txt" ]; then
  cp "$ROOT/saved/probes/restore-sequence.txt" "$ROOT/check/probes/"
fi
if [ -f "$ROOT/saved/probes/restore-views.txt" ]; then
  cp "$ROOT/saved/probes/restore-views.txt" "$ROOT/check/probes/"
fi
printf '%s\n' yes > "$ROOT/check/probes/full-gold.txt"
cp "$ROOT/saved/reports/"* "$ROOT/check/reports/"
docker exec -u 0 "$PGNAME" rm -rf /var/lib/postgresql/a377-check
docker exec -u 0 "$PGNAME" mkdir -p /var/lib/postgresql/a377-check
docker cp "$ROOT/check/." "$PGNAME":/var/lib/postgresql/a377-check/
set +e
docker exec -u 0 "$PGNAME" \
  /var/lib/postgresql/a377-glibc/ld-linux-aarch64.so.1 \
  --library-path /var/lib/postgresql/a377-glibc \
  /var/lib/postgresql/a377-work/cut-check \
  /var/lib/postgresql/a377-check > "$EVIDENCE/live/phases/cut-check.out" 2> "$EVIDENCE/live/phases/cut-check.err"
check_code=$?
set -e
printf '%s\n' "$check_code" > "$EVIDENCE/live/phases/cut-check.exit"
echo "phase cut-check exit ${check_code}"

sql_out postgres "$EVIDENCE/live/databases.txt" "SELECT datname FROM pg_database WHERE datname LIKE 'oo_a356_%' OR datname LIKE 'oo_a377_%' ORDER BY 1"
if grep -R -l -a -F -f "$ROOT/secrets/admin.pass" --exclude-dir=secrets "$EVIDENCE" > "$ROOT/password-leak-files.txt"; then
  cp "$ROOT/password-leak-files.txt" "$EVIDENCE/live/password-leak-files.txt"
  echo "password leaked into evidence" >&2
  exit 1
fi
printf '%s\n' none > "$EVIDENCE/live/password-leak-files.txt"
marker=$(printf '%s%s' '-----BEGIN PRI' 'VATE KEY-----')
if grep -R -l -a -F --exclude-dir=secrets -- "$marker" "$EVIDENCE" > "$ROOT/private-key-files.txt"; then
  cp "$ROOT/private-key-files.txt" "$EVIDENCE/live/private-key-files.txt"
  echo "private key leaked into evidence" >&2
  exit 1
fi
printf '%s\n' none > "$EVIDENCE/live/private-key-files.txt"
docker cp "$PGNAME":/var/lib/postgresql/a377-backup/manifest.json "$EVIDENCE/live/manifest.json"
docker cp "$PGNAME":/var/lib/postgresql/a377-backup/manifest.sha256 "$EVIDENCE/live/manifest.sha256"
docker cp "$PGNAME":/var/lib/postgresql/a377-backup/catalog.txt "$EVIDENCE/live/catalog.txt"
{
  printf 'backup=%s\n' "$(cat "$EVIDENCE/live/phases/backup.exit")"
  printf 'restore_corrupt_dump=%s\n' "$(cat "$EVIDENCE/live/phases/restore-corrupt-dump.exit")"
  printf 'restore=%s\n' "$(cat "$EVIDENCE/live/phases/restore.exit")"
  printf 'cut_check=%s\n' "$(cat "$EVIDENCE/live/phases/cut-check.exit")"
  printf 'load_fixture=%s\n' "$(cat "$EVIDENCE/live/phases/load-fixture.exit")"
  printf 'mark_post_cut=%s\n' "$(cat "$EVIDENCE/live/phases/mark-post-cut.exit")"
  printf 'postcut_absent=%s\n' "$(cat "$ROOT/saved/probes/postcut-absent.txt")"
  printf 'sequence_before=%s\n' "$(cat "$ROOT/saved/probes/sequence-before.txt")"
  printf 'sequence_after=%s\n' "$(cat "$ROOT/saved/probes/sequence-after.txt")"
  printf 'nextval_exit=%s\n' "$(cat "$ROOT/saved/probes/preexisting-nextval.exit")"
} > "$EVIDENCE/live/raw-exits.txt"
if [ "$check_code" -ne 0 ]; then
  echo "independent cut check failed" >&2
  exit 1
fi
printf '%s\n' 0 > "$EVIDENCE/live/run-live.exit"
echo "live cut finished"
