#!/bin/bash
# Load the 100k fixture, reject a value update while both SHARE locks are held,
# commit that update after release, restore, and refuse a mismatched dump.
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
    docker cp "$PGNAME":/var/lib/postgresql/a368-work/readiness.json "$EVIDENCE/live/phases/${name}-readiness.json" 2>/dev/null || true
    echo "phase ${name} expected ${expect}" >&2
    exit 1
  fi
}

sql_out() {
  db=$1
  dest=$2
  sql=$3
  printf '%s\n' "$sql" > "$ROOT/run/query.sql"
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a368-work/psql-exec.sh "$db" > "$dest"
}

sql_status() {
  db=$1
  dest_out=$2
  dest_err=$3
  dest_exit=$4
  set +e
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a368-work/psql-exec.sh "$db" > "$dest_out" 2> "$dest_err"
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

mkdir -p "$EVIDENCE/live/phases" "$ROOT/saved/dumps" "$ROOT/saved/gold" "$ROOT/saved/probes" "$ROOT/saved/reports" "$ROOT/check"
docker cp "$EVIDENCE/linux/psql-exec.sh" "$PGNAME":/var/lib/postgresql/a368-work/psql-exec.sh
docker exec -u 0 "$PGNAME" chmod 755 /var/lib/postgresql/a368-work/psql-exec.sh

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

sql_out oo_a356_agent "$ROOT/saved/probes/amount-before.raw" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"
sql_out oo_a356_control "$ROOT/saved/probes/marker-before.raw" "SELECT source_cut || '|' || entry_count::text FROM oo_key_fixture.cut_marker"
sql_out oo_a356_agent "$ROOT/saved/probes/count-before.raw" "SELECT count(*)::text FROM shop.entry"
trim_copy "$ROOT/saved/probes/marker-before.raw" "$ROOT/saved/probes/marker-before.txt"
trim_copy "$ROOT/saved/probes/count-before.raw" "$ROOT/saved/probes/count-before.txt"

cat > "$ROOT/run/query.sql" <<'EOF'
\set VERBOSITY verbose
SET lock_timeout = '2s';
UPDATE shop.entry SET amount = 1.0001 WHERE entry_id = 3;
EOF
sql_status oo_a356_agent "$ROOT/saved/probes/rejected.out" "$ROOT/saved/probes/rejected.err" "$ROOT/saved/probes/rejected.exit"
sql_out oo_a356_agent "$ROOT/saved/probes/amount-during-lock.raw" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"
trim_copy "$ROOT/saved/probes/amount-during-lock.raw" "$ROOT/saved/probes/amount-during-lock.txt"

set +e
wait "$backup_pid"
backup_code=$?
set -e
backup_end=$(date +%s)
printf '%s\n' "$backup_code" > "$EVIDENCE/live/phases/backup.exit"
printf 'backup_outer_start=%s\nbackup_outer_end=%s\n' "$backup_start" "$backup_end" > "$EVIDENCE/live/timing.txt"
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

phase mark-post-cut 0 mark-post-cut "$OPERATOR"

docker cp "$PGNAME":/var/lib/postgresql/a368-backup/manifest.json "$ROOT/saved/manifest.json"
docker cp "$PGNAME":/var/lib/postgresql/a368-backup/manifest.sha256 "$ROOT/saved/manifest.sha256"
docker cp "$PGNAME":/var/lib/postgresql/a368-backup/oo_a356_agent.dump "$ROOT/saved/dumps/oo_a356_agent.dump"
docker cp "$PGNAME":/var/lib/postgresql/a368-backup/oo_a356_control.dump "$ROOT/saved/dumps/oo_a356_control.dump"
docker cp "$PGNAME":/var/lib/postgresql/a368-backup/gold/shop-entry.copy "$ROOT/saved/gold/shop-entry.copy"
docker exec -u 0 "$PGNAME" sh -c 'rm -rf /var/lib/postgresql/a368-backup-good && mkdir -p /var/lib/postgresql/a368-backup-good && cp -a /var/lib/postgresql/a368-backup/. /var/lib/postgresql/a368-backup-good/'

list_restore_dbs "$ROOT/saved/probes/dbs-before-corrupt.txt"
docker exec -u 0 "$PGNAME" sh -c 'chmod u+w /var/lib/postgresql/a368-backup/oo_a356_agent.dump && dd if=/dev/zero of=/var/lib/postgresql/a368-backup/oo_a356_agent.dump bs=1 count=1 conv=notrunc'
phase restore-corrupt-dump 4 restore "$OPERATOR"
docker cp "$PGNAME":/var/lib/postgresql/a368-work/readiness.json "$EVIDENCE/live/corrupt-dump-readiness.json"
cp "$EVIDENCE/live/corrupt-dump-readiness.json" "$ROOT/saved/reports/corrupt-dump-readiness.json"
list_restore_dbs "$ROOT/saved/probes/dbs-after-corrupt.txt"

docker exec -u 0 "$PGNAME" sh -c 'rm -rf /var/lib/postgresql/a368-backup && mkdir -p /var/lib/postgresql/a368-backup && cp -a /var/lib/postgresql/a368-backup-good/. /var/lib/postgresql/a368-backup/'
restore_start=$(date +%s)
phase restore 0 restore "$OPERATOR"
restore_end=$(date +%s)
printf 'restore_outer_start=%s\nrestore_outer_end=%s\n' "$restore_start" "$restore_end" >> "$EVIDENCE/live/timing.txt"
docker cp "$PGNAME":/var/lib/postgresql/a368-work/readiness.json "$EVIDENCE/live/restore-readiness.json"
cp "$EVIDENCE/live/restore-readiness.json" "$ROOT/saved/reports/restore-readiness.json"
sql_out oo_a356_restore_agent "$ROOT/saved/probes/restore-amount.raw" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"
trim_copy "$ROOT/saved/probes/restore-amount.raw" "$ROOT/saved/probes/restore-amount.txt"
sql_out oo_a356_restore_agent "$EVIDENCE/live/phases/restore-post.txt" "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'"
sql_out oo_a356_agent "$EVIDENCE/live/phases/source-post.txt" "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'"

phase restore-existing 4 restore "$OPERATOR"
docker cp "$PGNAME":/var/lib/postgresql/a368-work/readiness.json "$EVIDENCE/live/restore-existing-readiness.json"
sql_out oo_a356_restore_agent "$EVIDENCE/live/phases/existing-amount.txt" "SELECT amount::text FROM shop.entry WHERE entry_id = 3"

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
cp "$ROOT/saved/reports/"* "$ROOT/check/reports/"
docker exec -u 0 "$PGNAME" rm -rf /var/lib/postgresql/a368-check
docker exec -u 0 "$PGNAME" mkdir -p /var/lib/postgresql/a368-check
docker cp "$ROOT/check/." "$PGNAME":/var/lib/postgresql/a368-check/
set +e
docker exec -u 0 "$PGNAME" \
  /var/lib/postgresql/a368-glibc/ld-linux-aarch64.so.1 \
  --library-path /var/lib/postgresql/a368-glibc \
  /var/lib/postgresql/a368-work/cut-check \
  /var/lib/postgresql/a368-check > "$EVIDENCE/live/phases/cut-check.out" 2> "$EVIDENCE/live/phases/cut-check.err"
check_code=$?
set -e
printf '%s\n' "$check_code" > "$EVIDENCE/live/phases/cut-check.exit"
echo "phase cut-check exit ${check_code}"

query_names() {
  sql_out postgres "$EVIDENCE/live/databases.txt" "SELECT datname FROM pg_database WHERE datname LIKE 'oo_a356_%' OR datname LIKE 'oo_a368_%' ORDER BY 1"
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
