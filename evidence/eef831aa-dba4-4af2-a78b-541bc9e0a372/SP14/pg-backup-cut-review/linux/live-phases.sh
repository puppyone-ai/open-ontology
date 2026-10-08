#!/bin/bash
# Load the 100k fixture, reject a value update while both SHARE locks are held,
# probe sequence/view/new-table DDL in that same window, then restore.
# Sourced by run-live.sh. Does not start PostgreSQL or a Rust builder.

if [ -z "${PGNAME:-}" ] || [ -z "${BIN:-}" ] || [ -z "${OPERATOR:-}" ]; then
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
    docker cp "$PGNAME":/var/lib/postgresql/a372-work/readiness.json "$EVIDENCE/live/phases/${name}-readiness.json" 2>/dev/null || true
    echo "phase ${name} expected ${expect}" >&2
    exit 1
  fi
}

sql_out() {
  db=$1
  dest=$2
  sql=$3
  printf '%s\n' "$sql" > "$ROOT/run/query.sql"
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a372-work/psql-exec.sh "$db" > "$dest"
}

sql_status() {
  db=$1
  dest_out=$2
  dest_err=$3
  dest_exit=$4
  set +e
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a372-work/psql-exec.sh "$db" > "$dest_out" 2> "$dest_err"
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

boundary_sql() {
  name=$1
  db=$2
  sql=$3
  printf '%s\n' "$sql" > "$ROOT/run/query.sql"
  sql_status "$db" \
    "$EVIDENCE/live/boundary/${name}.out" \
    "$EVIDENCE/live/boundary/${name}.err" \
    "$EVIDENCE/live/boundary/${name}.exit"
}

one_line() {
  if [ -s "$1" ]; then
    tr -d '[:space:]' < "$1"
  else
    printf none
  fi
}

mkdir -p "$EVIDENCE/live/phases" "$EVIDENCE/live/boundary" "$ROOT/saved/dumps" "$ROOT/saved/gold" "$ROOT/saved/probes" "$ROOT/saved/reports" "$ROOT/check"
docker cp "$EVIDENCE/linux/psql-exec.sh" "$PGNAME":/var/lib/postgresql/a372-work/psql-exec.sh
docker exec -u 0 "$PGNAME" chmod 755 /var/lib/postgresql/a372-work/psql-exec.sh

phase load-fixture 0 load-fixture "$OPERATOR"

backup_start=$(date +%s)
set +e
invoke backup "$OPERATOR" > "$EVIDENCE/live/phases/backup.out" 2> "$EVIDENCE/live/phases/backup.err" &
backup_pid=$!
set -e
printf '%s\n' "$backup_pid" > "$EVIDENCE/live/phases/backup.pid"
cleanup_backup() {
  if [ -n "${backup_pid:-}" ] && kill -0 "$backup_pid" 2>/dev/null; then
    kill "$backup_pid" 2>/dev/null || true
    wait "$backup_pid" 2>/dev/null || true
  fi
}
trap cleanup_backup EXIT

seen=0
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

# These files are staged before the cut. Parallel probes each get their own name
# so the value update can wait on SHARE while the DDL commits in the same window.
cat > "$ROOT/run/q-amount.sql" <<'EOF'
SELECT amount::text FROM shop.entry WHERE entry_id = 3;
EOF
cat > "$ROOT/run/q-marker.sql" <<'EOF'
SELECT source_cut || '|' || entry_count::text FROM oo_key_fixture.cut_marker;
EOF
cat > "$ROOT/run/q-count.sql" <<'EOF'
SELECT count(*)::text FROM shop.entry;
EOF
cat > "$ROOT/run/q-reject.sql" <<'EOF'
\set VERBOSITY verbose
SET lock_timeout = '1500ms';
UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3;
EOF
cat > "$ROOT/run/q-agent-vshare.sql" <<'EOF'
SELECT count(*)::text FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_agent' AND l.granted AND l.mode = 'ShareLock' AND c.relkind IN ('v','S','m');
EOF
cat > "$ROOT/run/q-control-vshare.sql" <<'EOF'
SELECT count(*)::text FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_control' AND l.granted AND l.mode = 'ShareLock' AND c.relkind IN ('v','S','m');
EOF
cat > "$ROOT/run/q-agent-ddl.sql" <<'EOF'
\set ON_ERROR_STOP off
\set VERBOSITY verbose
SET lock_timeout = '2s';
SELECT coalesce(string_agg(n.nspname || '.' || c.relname || ':' || c.relkind::text, ',' ORDER BY n.nspname, c.relname), 'none')
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname NOT IN ('pg_catalog','information_schema','pg_toast')
  AND c.relkind IN ('S','v','m');
DO $$
BEGIN
  COMMENT ON VIEW oo_dict.enums IS 'a372-during-cut';
  RAISE NOTICE 'step comment ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step comment %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE SCHEMA oo_a372_probe;
  RAISE NOTICE 'step schema ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step schema %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE TABLE oo_a372_probe.added (id integer primary key);
  RAISE NOTICE 'step table ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step table %', SQLSTATE;
END $$;
DO $$
BEGIN
  INSERT INTO oo_a372_probe.added (id) VALUES (7);
  RAISE NOTICE 'step insert ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step insert %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE SEQUENCE oo_a372_probe.seq;
  RAISE NOTICE 'step sequence ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step sequence %', SQLSTATE;
END $$;
DO $$
BEGIN
  PERFORM nextval('oo_a372_probe.seq');
  RAISE NOTICE 'step nextval ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step nextval %', SQLSTATE;
END $$;
DO $$
BEGIN
  CREATE VIEW oo_a372_probe.v AS SELECT 7 AS n;
  RAISE NOTICE 'step view ok';
EXCEPTION WHEN OTHERS THEN
  RAISE NOTICE 'step view %', SQLSTATE;
END $$;
EOF
cp "$ROOT/run/q-agent-ddl.sql" "$ROOT/run/q-control-ddl.sql"
# Control comments the calendar view. The other steps match the agent probe.
perl -0pi -e "s/oo_dict\\.enums/oo_dict.calendar/" "$ROOT/run/q-control-ddl.sql"

start_sql() {
  name=$1
  db=$2
  sqlfile=$3
  dest_out=$4
  dest_err=$5
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a372-work/psql-exec.sh "$db" "$sqlfile" > "$dest_out" 2> "$dest_err" &
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
start_sql amount oo_a356_agent q-amount.sql "$ROOT/saved/probes/amount-before.raw" "$ROOT/saved/probes/amount-before.err"
start_sql marker oo_a356_control q-marker.sql "$ROOT/saved/probes/marker-before.raw" "$ROOT/saved/probes/marker-before.err"
start_sql count oo_a356_agent q-count.sql "$ROOT/saved/probes/count-before.raw" "$ROOT/saved/probes/count-before.err"
start_sql reject oo_a356_agent q-reject.sql "$ROOT/saved/probes/rejected.out" "$ROOT/saved/probes/rejected.err"
start_sql agent-vshare oo_a356_agent q-agent-vshare.sql "$EVIDENCE/live/boundary/agent-view-share.out" "$EVIDENCE/live/boundary/agent-view-share.err"
start_sql control-vshare oo_a356_control q-control-vshare.sql "$EVIDENCE/live/boundary/control-view-share.out" "$EVIDENCE/live/boundary/control-view-share.err"
start_sql agent-ddl oo_a356_agent q-agent-ddl.sql "$EVIDENCE/live/boundary/agent-ddl.out" "$EVIDENCE/live/boundary/agent-ddl.err"
start_sql control-ddl oo_a356_control q-control-ddl.sql "$EVIDENCE/live/boundary/control-ddl.out" "$EVIDENCE/live/boundary/control-ddl.err"
finish_sql amount "$ROOT/saved/probes/amount-before.exit"
finish_sql marker "$ROOT/saved/probes/marker-before.exit"
finish_sql count "$ROOT/saved/probes/count-before.exit"
finish_sql agent-vshare "$EVIDENCE/live/boundary/agent-view-share.exit"
finish_sql control-vshare "$EVIDENCE/live/boundary/control-view-share.exit"
finish_sql agent-ddl "$EVIDENCE/live/boundary/agent-ddl.exit"
finish_sql control-ddl "$EVIDENCE/live/boundary/control-ddl.exit"
ddl_done=$(date +%s)
sql_out oo_a356_agent "$EVIDENCE/live/boundary/agent-lock-after.out" "SELECT count(*)::text FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_agent' AND l.granted AND l.mode = 'ShareLock' AND n.nspname = 'shop' AND c.relname = 'entry'"
sql_out oo_a356_control "$EVIDENCE/live/boundary/control-lock-after.out" "SELECT count(*)::text FROM pg_locks l JOIN pg_class c ON c.oid = l.relation JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_stat_activity a ON a.pid = l.pid WHERE a.application_name = 'oo_a356_cut_control' AND l.granted AND l.mode = 'ShareLock' AND n.nspname = 'oo_key_fixture' AND c.relname = 'cut_marker'"
finish_sql reject "$ROOT/saved/probes/rejected.exit"
reject_done=$(date +%s)
printf 'probe_start=%s\nprobe_ddl_done=%s\nprobe_reject_done=%s\n' "$probe_start" "$ddl_done" "$reject_done" > "$EVIDENCE/live/boundary/probe-timing.txt"
trim_copy "$ROOT/saved/probes/marker-before.raw" "$ROOT/saved/probes/marker-before.txt"
trim_copy "$ROOT/saved/probes/count-before.raw" "$ROOT/saved/probes/count-before.txt"
sql_out oo_a356_agent "$ROOT/saved/probes/amount-during-lock.raw" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"
trim_copy "$ROOT/saved/probes/amount-during-lock.raw" "$ROOT/saved/probes/amount-during-lock.txt"
if ! grep -q '55P03' "$ROOT/saved/probes/rejected.err"; then
  echo "value update was not rejected while the cut was in progress" >&2
  exit 1
fi

set +e
wait "$backup_pid"
backup_code=$?
set -e
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

boundary_sql agent-source-comment oo_a356_agent "SELECT coalesce(obj_description('oo_dict.enums'::regclass), 'none')"
boundary_sql control-source-comment oo_a356_control "SELECT coalesce(obj_description('oo_dict.calendar'::regclass), 'none')"
boundary_sql agent-source-added oo_a356_agent "SELECT count(*)::text FROM oo_a372_probe.added"
boundary_sql control-source-added oo_a356_control "SELECT count(*)::text FROM oo_a372_probe.added"
boundary_sql agent-source-seq oo_a356_agent "SELECT last_value::text FROM oo_a372_probe.seq"
boundary_sql control-source-seq oo_a356_control "SELECT last_value::text FROM oo_a372_probe.seq"
boundary_sql agent-source-view oo_a356_agent "SELECT n::text FROM oo_a372_probe.v"
boundary_sql control-source-view oo_a356_control "SELECT n::text FROM oo_a372_probe.v"

phase mark-post-cut 0 mark-post-cut "$OPERATOR"

docker cp "$PGNAME":/var/lib/postgresql/a372-backup/manifest.json "$ROOT/saved/manifest.json"
docker cp "$PGNAME":/var/lib/postgresql/a372-backup/manifest.sha256 "$ROOT/saved/manifest.sha256"
docker cp "$PGNAME":/var/lib/postgresql/a372-backup/oo_a356_agent.dump "$ROOT/saved/dumps/oo_a356_agent.dump"
docker cp "$PGNAME":/var/lib/postgresql/a372-backup/oo_a356_control.dump "$ROOT/saved/dumps/oo_a356_control.dump"
docker cp "$PGNAME":/var/lib/postgresql/a372-backup/gold/. "$ROOT/saved/gold/"
docker exec -u 0 "$PGNAME" sh -c 'rm -rf /var/lib/postgresql/a372-backup-good && mkdir -p /var/lib/postgresql/a372-backup-good && cp -a /var/lib/postgresql/a372-backup/. /var/lib/postgresql/a372-backup-good/'

list_restore_dbs "$ROOT/saved/probes/dbs-before-corrupt.txt"
docker exec -u 0 "$PGNAME" sh -c 'chmod u+w /var/lib/postgresql/a372-backup/oo_a356_agent.dump && dd if=/dev/zero of=/var/lib/postgresql/a372-backup/oo_a356_agent.dump bs=1 count=1 conv=notrunc'
phase restore-corrupt-dump 4 restore "$OPERATOR"
docker cp "$PGNAME":/var/lib/postgresql/a372-work/readiness.json "$EVIDENCE/live/corrupt-dump-readiness.json"
cp "$EVIDENCE/live/corrupt-dump-readiness.json" "$ROOT/saved/reports/corrupt-dump-readiness.json"
list_restore_dbs "$ROOT/saved/probes/dbs-after-corrupt.txt"

docker exec -u 0 "$PGNAME" sh -c 'rm -rf /var/lib/postgresql/a372-backup && mkdir -p /var/lib/postgresql/a372-backup && cp -a /var/lib/postgresql/a372-backup-good/. /var/lib/postgresql/a372-backup/'
restore_start=$(date +%s)
phase restore 0 restore "$OPERATOR"
restore_end=$(date +%s)
printf 'restore_outer_start=%s\nrestore_outer_end=%s\n' "$restore_start" "$restore_end" >> "$EVIDENCE/live/timing.txt"
docker cp "$PGNAME":/var/lib/postgresql/a372-work/readiness.json "$EVIDENCE/live/restore-readiness.json"
cp "$EVIDENCE/live/restore-readiness.json" "$ROOT/saved/reports/restore-readiness.json"
sql_out oo_a356_restore_agent "$ROOT/saved/probes/restore-amount.raw" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"
trim_copy "$ROOT/saved/probes/restore-amount.raw" "$ROOT/saved/probes/restore-amount.txt"
sql_out oo_a356_restore_agent "$EVIDENCE/live/phases/restore-post.txt" "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'"
sql_out oo_a356_agent "$EVIDENCE/live/phases/source-post.txt" "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'"

boundary_sql agent-restore-comment oo_a356_restore_agent "SELECT coalesce(obj_description('oo_dict.enums'::regclass), 'none')"
boundary_sql control-restore-comment oo_a356_restore_control "SELECT coalesce(obj_description('oo_dict.calendar'::regclass), 'none')"
boundary_sql agent-restore-added oo_a356_restore_agent "SELECT count(*)::text FROM oo_a372_probe.added"
boundary_sql control-restore-added oo_a356_restore_control "SELECT count(*)::text FROM oo_a372_probe.added"
boundary_sql agent-restore-seq oo_a356_restore_agent "SELECT last_value::text FROM oo_a372_probe.seq"
boundary_sql control-restore-seq oo_a356_restore_control "SELECT last_value::text FROM oo_a372_probe.seq"
boundary_sql agent-restore-view oo_a356_restore_agent "SELECT n::text FROM oo_a372_probe.v"
boundary_sql control-restore-view oo_a356_restore_control "SELECT n::text FROM oo_a372_probe.v"

phase restore-existing 4 restore "$OPERATOR"
docker cp "$PGNAME":/var/lib/postgresql/a372-work/readiness.json "$EVIDENCE/live/restore-existing-readiness.json"
sql_out oo_a356_restore_agent "$EVIDENCE/live/phases/existing-amount.txt" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"

{
  printf 'agent_view_share=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-view-share.out")"
  printf 'control_view_share=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-view-share.out")"
  printf 'agent_ddl_exit=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-ddl.exit")"
  printf 'control_ddl_exit=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-ddl.exit")"
  printf 'agent_lock_after=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-lock-after.out")"
  printf 'control_lock_after=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-lock-after.out")"
  printf 'agent_ddl_out=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-ddl.out")"
  printf 'control_ddl_out=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-ddl.out")"
  printf 'agent_ddl_err=%s\n' "$(tr '\n' ' ' < "$EVIDENCE/live/boundary/agent-ddl.err" | tr -s ' ')"
  printf 'control_ddl_err=%s\n' "$(tr '\n' ' ' < "$EVIDENCE/live/boundary/control-ddl.err" | tr -s ' ')"
  printf 'agent_source_comment=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-source-comment.out")"
  printf 'control_source_comment=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-source-comment.out")"
  printf 'agent_source_added=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-source-added.out")"
  printf 'control_source_added=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-source-added.out")"
  printf 'agent_source_seq=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-source-seq.out")"
  printf 'control_source_seq=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-source-seq.out")"
  printf 'agent_source_view=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-source-view.out")"
  printf 'control_source_view=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-source-view.out")"
  printf 'agent_restore_comment=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-restore-comment.out")"
  printf 'control_restore_comment=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-restore-comment.out")"
  printf 'agent_restore_added_exit=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-restore-added.exit")"
  printf 'agent_restore_added=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-restore-added.out")"
  printf 'control_restore_added_exit=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-restore-added.exit")"
  printf 'control_restore_added=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-restore-added.out")"
  printf 'agent_restore_seq=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-restore-seq.out")"
  printf 'control_restore_seq=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-restore-seq.out")"
  printf 'agent_restore_view=%s\n' "$(one_line "$EVIDENCE/live/boundary/agent-restore-view.out")"
  printf 'control_restore_view=%s\n' "$(one_line "$EVIDENCE/live/boundary/control-restore-view.out")"
} > "$EVIDENCE/live/boundary/summary.txt"

rm -rf "$ROOT/check"
mkdir -p "$ROOT/check/dumps" "$ROOT/check/gold" "$ROOT/check/probes" "$ROOT/check/reports"
cp "$ROOT/saved/manifest.json" "$ROOT/saved/manifest.sha256" "$ROOT/check/"
cp "$ROOT/saved/dumps/"* "$ROOT/check/dumps/"
cp "$ROOT/saved/gold/"* "$ROOT/check/gold/"
cp "$ROOT/saved/probes/rejected.err" "$ROOT/saved/probes/rejected.exit" "$ROOT/saved/probes/committed.exit" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/marker-before.txt" "$ROOT/saved/probes/marker-after.txt" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/count-before.txt" "$ROOT/saved/probes/count-after.txt" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/amount-during-lock.txt" "$ROOT/saved/probes/source-amount.txt" "$ROOT/saved/probes/restore-amount.txt" "$ROOT/check/probes/"
cp "$ROOT/saved/probes/dbs-before-corrupt.txt" "$ROOT/saved/probes/dbs-after-corrupt.txt" "$ROOT/saved/probes/cut-sessions-after.txt" "$ROOT/check/probes/"
printf '%s\n' yes > "$ROOT/check/probes/full-gold.txt"
cp "$ROOT/saved/reports/"* "$ROOT/check/reports/"
docker exec -u 0 "$PGNAME" rm -rf /var/lib/postgresql/a372-check
docker exec -u 0 "$PGNAME" mkdir -p /var/lib/postgresql/a372-check
docker cp "$ROOT/check/." "$PGNAME":/var/lib/postgresql/a372-check/
set +e
docker exec -u 0 "$PGNAME" \
  /var/lib/postgresql/a372-glibc/ld-linux-aarch64.so.1 \
  --library-path /var/lib/postgresql/a372-glibc \
  /var/lib/postgresql/a372-work/cut-check \
  /var/lib/postgresql/a372-check > "$EVIDENCE/live/phases/cut-check.out" 2> "$EVIDENCE/live/phases/cut-check.err"
check_code=$?
set -e
printf '%s\n' "$check_code" > "$EVIDENCE/live/phases/cut-check.exit"
echo "phase cut-check exit ${check_code}"

query_names() {
  sql_out postgres "$EVIDENCE/live/databases.txt" "SELECT datname FROM pg_database WHERE datname LIKE 'oo_a356_%' OR datname LIKE 'oo_a372_%' ORDER BY 1"
}
query_names
if grep -R -l -a -F -f "$ROOT/secrets/admin.pass" "$EVIDENCE" > "$ROOT/password-leak-files.txt"; then
  cp "$ROOT/password-leak-files.txt" "$EVIDENCE/live/password-leak-files.txt"
  echo "password leaked into evidence" >&2
  exit 1
fi
printf '%s\n' none > "$EVIDENCE/live/password-leak-files.txt"
marker=$(printf '%s%s' '-----BEGIN PRI' 'VATE KEY-----')
if grep -R -l -a -F -- "$marker" "$EVIDENCE" > "$ROOT/private-key-files.txt"; then
  cp "$ROOT/private-key-files.txt" "$EVIDENCE/live/private-key-files.txt"
  echo "private key leaked into evidence" >&2
  exit 1
fi
printf '%s\n' none > "$EVIDENCE/live/private-key-files.txt"
cp "$EVIDENCE/live/manifest-placeholder" "$EVIDENCE/live/manifest.json" 2>/dev/null || true
docker cp "$PGNAME":/var/lib/postgresql/a372-backup/manifest.json "$EVIDENCE/live/manifest.json"
docker cp "$PGNAME":/var/lib/postgresql/a372-backup/manifest.sha256 "$EVIDENCE/live/manifest.sha256"
{
  printf 'backup=%s\n' "$(cat "$EVIDENCE/live/phases/backup.exit")"
  printf 'restore_corrupt_dump=%s\n' "$(cat "$EVIDENCE/live/phases/restore-corrupt-dump.exit")"
  printf 'restore=%s\n' "$(cat "$EVIDENCE/live/phases/restore.exit")"
  printf 'restore_existing=%s\n' "$(cat "$EVIDENCE/live/phases/restore-existing.exit")"
  printf 'cut_check=%s\n' "$(cat "$EVIDENCE/live/phases/cut-check.exit")"
  printf 'load_fixture=%s\n' "$(cat "$EVIDENCE/live/phases/load-fixture.exit")"
  printf 'mark_post_cut=%s\n' "$(cat "$EVIDENCE/live/phases/mark-post-cut.exit")"
} > "$EVIDENCE/live/raw-exits.txt"
if [ "$check_code" -ne 0 ]; then
  echo "independent cut check failed" >&2
  exit 1
fi
printf '%s\n' 0 > "$EVIDENCE/live/run-live.exit"
echo "live cut finished"
