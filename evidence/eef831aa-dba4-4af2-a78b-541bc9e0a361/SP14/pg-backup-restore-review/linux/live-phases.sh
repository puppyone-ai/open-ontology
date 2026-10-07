#!/bin/bash
# Load, backup, refusal probes, fresh restore, then the independent oracle.
# Sourced after install_private_glibc. Does not start PostgreSQL or a builder.

if [ -z "${PGNAME:-}" ] || [ -z "${BIN:-}" ] || [ -z "${OPERATOR:-}" ]; then
  echo "live-phases.sh is sourced by run-all.sh" >&2
  exit 2
fi

finish_oracle_tail() {
  docker exec -u 0 "$PGNAME" mkdir -p /var/lib/postgresql/a361-oracle/shop
  printf '%s\n' "\\copy (SELECT party_key, display_name, active, phone, national_id FROM shop.party ORDER BY party_key) TO '/var/lib/postgresql/a361-oracle/shop/party.copy'" > "$ROOT/run/query.sql"
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a361-work/psql-exec.sh oo_a356_restore_agent > "$EVIDENCE/live/phases/copy-party.out" 2> "$EVIDENCE/live/phases/copy-party.err"
  printf '%s\n' "\\copy (SELECT account_id, party_key, opened, label FROM shop.account ORDER BY account_id) TO '/var/lib/postgresql/a361-oracle/shop/account.copy'" > "$ROOT/run/query.sql"
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a361-work/psql-exec.sh oo_a356_restore_agent > "$EVIDENCE/live/phases/copy-account.out" 2> "$EVIDENCE/live/phases/copy-account.err"
  printf '%s\n' "\\copy (SELECT entry_id, account_id, qty, amount, memo FROM shop.entry ORDER BY entry_id) TO '/var/lib/postgresql/a361-oracle/shop/entry.copy'" > "$ROOT/run/query.sql"
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a361-work/psql-exec.sh oo_a356_restore_agent > "$EVIDENCE/live/phases/copy-entry.out" 2> "$EVIDENCE/live/phases/copy-entry.err"
  docker cp "$PGNAME":/var/lib/postgresql/a361-oracle/shop/party.copy "$ROOT/oracle/shop/party.copy"
  docker cp "$PGNAME":/var/lib/postgresql/a361-oracle/shop/account.copy "$ROOT/oracle/shop/account.copy"
  docker cp "$PGNAME":/var/lib/postgresql/a361-oracle/shop/entry.copy "$ROOT/oracle/shop/entry.copy"

  query_out postgres "$EVIDENCE/live/phases/schema-create.out" "CREATE DATABASE oo_a361_schema_probe TEMPLATE template0 ENCODING 'UTF8'"
  set +e
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a361-work/psql-script.sh oo_a361_schema_probe /schemas/tools/calendar-store/migrations/oo_calendar_control_v1.sql > "$EVIDENCE/live/phases/schema-calendar.out" 2> "$EVIDENCE/live/phases/schema-calendar.err"
  cal_code=$?
  set -e
  printf '%s\n' "$cal_code" > "$EVIDENCE/live/phases/schema-calendar.exit"
  if [ "$cal_code" -ne 0 ]; then
    echo "calendar schema probe exit ${cal_code}" >&2
    exit 1
  fi
  set +e
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a361-work/psql-script.sh oo_a361_schema_probe /schemas/tools/enum-store/migrations/oo_enum_control_v1.sql > "$ROOT/oracle/schema-probe/stdout.txt" 2> "$ROOT/oracle/schema-probe/stderr.txt"
  schema_code=$?
  set -e
  printf '%s\n' "$schema_code" > "$ROOT/oracle/schema-probe/exit.txt"
  cp "$ROOT/oracle/schema-probe/stderr.txt" "$EVIDENCE/live/phases/schema-enum.err"
  printf '%s\n' "$schema_code" > "$EVIDENCE/live/phases/schema-enum.exit"
  query_out postgres "$EVIDENCE/live/phases/schema-drop.out" "DROP DATABASE oo_a361_schema_probe"

  cp "$EVIDENCE/live/restore-readiness.json" "$ROOT/oracle/reports/restore-readiness.json"
  cp "$EVIDENCE/live/restore-missing-readiness.json" "$ROOT/oracle/reports/restore-missing-readiness.json"
  cp "$EVIDENCE/live/restore-corrupt-dump-readiness.json" "$ROOT/oracle/reports/restore-corrupt-dump-readiness.json"
  cp "$EVIDENCE/live/restore-corrupt-manifest-readiness.json" "$ROOT/oracle/reports/restore-corrupt-manifest-readiness.json"
  cp "$EVIDENCE/live/restore-existing-readiness.json" "$ROOT/oracle/reports/restore-existing-readiness.json"
  docker exec -u 0 "$PGNAME" rm -rf /var/lib/postgresql/a361-oracle
  docker exec -u 0 "$PGNAME" mkdir -p /var/lib/postgresql/a361-oracle
  docker cp "$ROOT/oracle/." "$PGNAME":/var/lib/postgresql/a361-oracle/
  set +e
  docker exec -u 0 "$PGNAME" \
    /var/lib/postgresql/a361-glibc/ld-linux-aarch64.so.1 \
    --library-path /var/lib/postgresql/a361-glibc \
    /var/lib/postgresql/a361-work/independent-gold \
    /var/lib/postgresql/a361-oracle > "$EVIDENCE/live/phases/oracle.out" 2> "$EVIDENCE/live/phases/oracle.err"
  oracle_code=$?
  set -e
  printf '%s\n' "$oracle_code" > "$EVIDENCE/live/phases/oracle.exit"
  docker cp "$PGNAME":/var/lib/postgresql/a361-oracle/verdict.txt "$ROOT/oracle/verdict.txt"

  query_out postgres "$EVIDENCE/live/databases.txt" "SELECT datname FROM pg_database WHERE datname LIKE 'oo_a356_%' OR datname = 'oo_a361_schema_probe' ORDER BY 1"

  rm -rf "$EVIDENCE/live/independent"
  mkdir -p "$EVIDENCE/live/independent"
  cp -R "$ROOT/oracle/." "$EVIDENCE/live/independent/"
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
  printf '%s\n' "$oracle_code" > "$EVIDENCE/live/run-all.exit"
  if [ "$oracle_code" -ne 0 ]; then
    echo "independent oracle exit ${oracle_code}" >&2
    exit 1
  fi
  echo "live restore finished"
}

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
    echo "phase ${name} expected ${expect}" >&2
    exit 1
  fi
}

copy_report() {
  name=$1
  docker cp "$PGNAME":/var/lib/postgresql/a361-work/readiness.json "$EVIDENCE/live/${name}-readiness.json"
}

query_out() {
  db=$1
  dest=$2
  sql=$3
  printf '%s\n' "$sql" > "$ROOT/run/query.sql"
  docker exec -u 0 "$PGNAME" /var/lib/postgresql/a361-work/psql-exec.sh "$db" > "$dest"
}

none_if_empty() {
  if [ ! -s "$1" ]; then
    printf '%s\n' none > "$1"
  fi
}

list_restore_dbs() {
  dest=$1
  query_out postgres "$dest" "SELECT datname FROM pg_database WHERE datname IN ('oo_a356_restore_control','oo_a356_restore_agent') ORDER BY 1"
  none_if_empty "$dest"
}

if [ "${A361_RESUME_FROM_COPY:-}" = "1" ]; then
  finish_oracle_tail
  return 0
fi

mkdir -p "$EVIDENCE/live/phases" "$ROOT/oracle/probes" "$ROOT/oracle/shop" "$ROOT/oracle/dumps" "$ROOT/oracle/reports" "$ROOT/oracle/schemas" "$ROOT/oracle/schema-probe"
cp "$REPO/tools/cdc-offset-store/migrations/oo_cdc_offset_control_v1.sql" "$ROOT/oracle/schemas/cdc.sql"
cp "$REPO/tools/session-store/migrations/oo_session_control_v1.sql" "$ROOT/oracle/schemas/session.sql"
cp "$REPO/tools/calendar-store/migrations/oo_calendar_control_v1.sql" "$ROOT/oracle/schemas/calendar.sql"
cp "$REPO/tools/enum-store/migrations/oo_enum_control_v1.sql" "$ROOT/oracle/schemas/enum.sql"
docker cp "$EVIDENCE/linux/psql-exec.sh" "$PGNAME":/var/lib/postgresql/a361-work/psql-exec.sh
docker cp "$EVIDENCE/linux/psql-script.sh" "$PGNAME":/var/lib/postgresql/a361-work/psql-script.sh
docker exec -u 0 "$PGNAME" chmod 755 /var/lib/postgresql/a361-work/psql-exec.sh /var/lib/postgresql/a361-work/psql-script.sh

phase load-fixture 0 load-fixture "$OPERATOR"

backup_start=$(date +%s)
phase backup 0 backup "$OPERATOR"
backup_end=$(date +%s)
printf 'backup_outer_start=%s\nbackup_outer_end=%s\n' "$backup_start" "$backup_end" > "$EVIDENCE/live/timing.txt"
docker cp "$PGNAME":/var/lib/postgresql/a361-backup/manifest.json "$ROOT/oracle/manifest.json"
docker cp "$PGNAME":/var/lib/postgresql/a361-backup/manifest.sha256 "$ROOT/oracle/manifest.sha256"
docker cp "$PGNAME":/var/lib/postgresql/a361-backup/oo_a356_agent.dump "$ROOT/oracle/dumps/oo_a356_agent.dump"
docker cp "$PGNAME":/var/lib/postgresql/a361-backup/oo_a356_control.dump "$ROOT/oracle/dumps/oo_a356_control.dump"
docker exec -u 0 "$PGNAME" /usr/bin/pg_restore -l /var/lib/postgresql/a361-backup/oo_a356_control.dump > "$EVIDENCE/live/control-toc.txt"
docker exec -u 0 "$PGNAME" /usr/bin/pg_restore -l /var/lib/postgresql/a361-backup/oo_a356_agent.dump > "$EVIDENCE/live/agent-toc.txt"

phase mark-post-cut 0 mark-post-cut "$OPERATOR"
docker exec -u 0 "$PGNAME" sh -c 'rm -rf /var/lib/postgresql/a361-backup-good && mkdir -p /var/lib/postgresql/a361-backup-good && cp -a /var/lib/postgresql/a361-backup/. /var/lib/postgresql/a361-backup-good/'

docker exec -u 0 "$PGNAME" rm -f /var/lib/postgresql/a361-backup/manifest.json /var/lib/postgresql/a361-backup/manifest.sha256
phase restore-missing 4 restore "$OPERATOR"
copy_report restore-missing
list_restore_dbs "$ROOT/oracle/probes/missing_dbs.txt"

docker exec -u 0 "$PGNAME" sh -c 'cp -a /var/lib/postgresql/a361-backup-good/. /var/lib/postgresql/a361-backup/'
docker exec -u 0 "$PGNAME" sh -c 'chmod u+w /var/lib/postgresql/a361-backup/oo_a356_agent.dump && dd if=/dev/zero of=/var/lib/postgresql/a361-backup/oo_a356_agent.dump bs=1 count=1 conv=notrunc'
phase restore-corrupt-dump 4 restore "$OPERATOR"
copy_report restore-corrupt-dump
list_restore_dbs "$ROOT/oracle/probes/corrupt_dump_dbs.txt"

docker exec -u 0 "$PGNAME" sh -c 'cp -a /var/lib/postgresql/a361-backup-good/. /var/lib/postgresql/a361-backup/'
docker exec -u 0 "$PGNAME" sh -c 'chmod u+w /var/lib/postgresql/a361-backup/manifest.json && dd if=/dev/zero of=/var/lib/postgresql/a361-backup/manifest.json bs=1 count=1 conv=notrunc'
phase restore-corrupt-manifest 4 restore "$OPERATOR"
copy_report restore-corrupt-manifest
list_restore_dbs "$ROOT/oracle/probes/corrupt_manifest_dbs.txt"

docker exec -u 0 "$PGNAME" sh -c 'cp -a /var/lib/postgresql/a361-backup-good/. /var/lib/postgresql/a361-backup/'
restore_start=$(date +%s)
phase restore 0 restore "$OPERATOR"
restore_end=$(date +%s)
printf 'restore_outer_start=%s\nrestore_outer_end=%s\n' "$restore_start" "$restore_end" >> "$EVIDENCE/live/timing.txt"
copy_report restore

query_out oo_a356_restore_agent "$ROOT/oracle/probes/before_existing_party.txt" "SELECT party_key || '|' || COALESCE(display_name, '<NULL>') FROM shop.party WHERE party_key = 'p00001'"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/before_existing_entry.txt" "SELECT count(*)::text FROM shop.entry"

phase restore-existing 4 restore "$OPERATOR"
copy_report restore-existing
query_out oo_a356_restore_agent "$ROOT/oracle/probes/existing_entry.txt" "SELECT count(*)::text FROM shop.entry"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/existing_party.txt" "SELECT party_key || '|' || COALESCE(display_name, '<NULL>') FROM shop.party WHERE party_key = 'p00001'"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/existing_post.txt" "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'"

query_out oo_a356_restore_agent "$ROOT/oracle/probes/party.txt" "SELECT count(*)::text FROM shop.party"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/account.txt" "SELECT count(*)::text FROM shop.account"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/entry.txt" "SELECT count(*)::text FROM shop.entry"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/orphans.txt" "SELECT ((SELECT count(*) FROM shop.account a LEFT JOIN shop.party p ON p.party_key = a.party_key WHERE p.party_key IS NULL) + (SELECT count(*) FROM shop.entry e LEFT JOIN shop.account a ON a.account_id = e.account_id WHERE a.account_id IS NULL))::text"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/null_display.txt" "SELECT CASE WHEN display_name IS NULL THEN 'null' ELSE 'value' END FROM shop.party WHERE party_key = 'p00000'"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/null_amounts.txt" "SELECT count(*)::text FROM shop.entry WHERE amount IS NULL"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/comment.txt" "SELECT obj_description('shop.party'::regclass)"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/amounts.txt" "SELECT entry_id::text || '|' || COALESCE(amount::text, '<NULL>') || '|' || qty::text FROM shop.entry WHERE entry_id BETWEEN 0 AND 6 ORDER BY entry_id"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/enum.txt" "SELECT code || '|' || status FROM oo_enum_control.published ORDER BY line_no"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/post_party_restore.txt" "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'"
query_out oo_a356_restore_control "$ROOT/oracle/probes/watermark.txt" "SELECT applied_pos::text || '|' || applied_txn_id || '|' || declared_next_txn || '|' || declared_next_pos::text FROM oo_cdc_offset.watermark"
query_out oo_a356_restore_control "$ROOT/oracle/probes/closure.txt" "SELECT closure_date::text || '|' || closure_label FROM oo_calendar_control.snapshot"
query_out oo_a356_restore_control "$ROOT/oracle/probes/published.txt" "SELECT last_closed_period_end_date::text || '|' || last_closed_period_end_midnight_status FROM oo_calendar_control.published"
query_out oo_a356_restore_control "$ROOT/oracle/probes/session.txt" "SELECT revision::text FROM oo_session_control.session_head WHERE session_id = 'sess-a356-1'"
query_out oo_a356_restore_control "$ROOT/oracle/probes/session_id.txt" "SELECT session_id FROM oo_session_control.session_head WHERE session_id = 'sess-a356-1'"
query_out oo_a356_restore_control "$ROOT/oracle/probes/keys.txt" "SELECT entity_key || '|' || source_member || '|' || issued_generation::text FROM oo_key_fixture.issued_key WHERE entity_key IN ('k-0001','k-0002') ORDER BY entity_key"
query_out oo_a356_restore_control "$ROOT/oracle/probes/negative.txt" "SELECT count(*)::text FROM oo_key_fixture.negative_decision WHERE left_key = 'k-0001' AND right_key = 'k-0002' AND decision = 'negative'"
query_out oo_a356_restore_control "$ROOT/oracle/probes/post_key_restore.txt" "SELECT count(*)::text FROM oo_key_fixture.issued_key WHERE entity_key = 'k-9000'"
query_out oo_a356_agent "$ROOT/oracle/probes/post_party_source.txt" "SELECT count(*)::text FROM shop.party WHERE party_key = 'p90000'"
query_out oo_a356_control "$ROOT/oracle/probes/post_key_source.txt" "SELECT count(*)::text FROM oo_key_fixture.issued_key WHERE entity_key = 'k-9000'"
query_out oo_a356_agent "$ROOT/oracle/probes/source_party.txt" "SELECT count(*)::text FROM shop.party"
query_out oo_a356_control "$ROOT/oracle/probes/source_keys.txt" "SELECT count(*)::text FROM oo_key_fixture.issued_key"
query_out oo_a356_restore_control "$ROOT/oracle/probes/cdc_sha.txt" "SELECT sql_sha256 FROM oo_cdc_offset.install_manifest"
query_out oo_a356_restore_control "$ROOT/oracle/probes/session_sha.txt" "SELECT sql_sha256 FROM oo_session_control.install_manifest"
query_out oo_a356_restore_control "$ROOT/oracle/probes/calendar_sha.txt" "SELECT sql_sha256 FROM oo_calendar_control.install_manifest"
query_out oo_a356_restore_agent "$ROOT/oracle/probes/enum_sha.txt" "SELECT sql_sha256 FROM oo_enum_control.install_manifest"

finish_oracle_tail
