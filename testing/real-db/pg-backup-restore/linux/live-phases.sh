#!/bin/bash
# Live load, backup, negative restores, and fresh-database restore.
# Sourced after install_private_glibc. Does not start PostgreSQL or a builder.

if [ -z "${PGNAME:-}" ] || [ -z "${BIN:-}" ] || [ -z "${OPERATOR:-}" ]; then
  echo "live-phases.sh is sourced by run-all.sh or continue-live.sh" >&2
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
    echo "phase ${name} expected ${expect}" >&2
    exit 1
  fi
}

copy_report() {
  name=$1
  docker cp "$PGNAME":/var/lib/postgresql/a356-work/readiness.json "$EVIDENCE/live/${name}-readiness.json"
  docker cp "$PGNAME":/var/lib/postgresql/a356-work/tool-error.txt "$EVIDENCE/live/${name}-tool-error.txt" 2>/dev/null || true
}

phase load-fixture 0 load-fixture "$OPERATOR"
phase backup 0 backup "$OPERATOR"
docker cp "$PGNAME":/var/lib/postgresql/a356-backup/manifest.json "$EVIDENCE/live/manifest.json"
docker cp "$PGNAME":/var/lib/postgresql/a356-backup/manifest.sha256 "$EVIDENCE/live/manifest.sha256"
phase mark-post-cut 0 mark-post-cut "$OPERATOR"
docker exec -u 0 "$PGNAME" sh -c 'rm -rf /var/lib/postgresql/a356-backup-good && mkdir -p /var/lib/postgresql/a356-backup-good && cp -a /var/lib/postgresql/a356-backup/. /var/lib/postgresql/a356-backup-good/'

docker exec -u 0 "$PGNAME" rm -f /var/lib/postgresql/a356-backup/manifest.json /var/lib/postgresql/a356-backup/manifest.sha256
phase restore-missing 4 restore "$OPERATOR"
copy_report restore-missing
grep -q '"replay_ready": false' "$EVIDENCE/live/restore-missing-readiness.json"
grep -q '"reason": "manifest_absent"' "$EVIDENCE/live/restore-missing-readiness.json"

docker exec -u 0 "$PGNAME" sh -c 'cp -a /var/lib/postgresql/a356-backup-good/. /var/lib/postgresql/a356-backup/'
docker exec -u 0 "$PGNAME" sh -c 'chmod u+w /var/lib/postgresql/a356-backup/oo_a356_agent.dump && dd if=/dev/zero of=/var/lib/postgresql/a356-backup/oo_a356_agent.dump bs=1 count=1 conv=notrunc'
phase restore-corrupt-dump 4 restore "$OPERATOR"
copy_report restore-corrupt-dump
grep -q '"replay_ready": false' "$EVIDENCE/live/restore-corrupt-dump-readiness.json"
grep -q '"reason": "dump_hash_mismatch"' "$EVIDENCE/live/restore-corrupt-dump-readiness.json"

docker exec -u 0 "$PGNAME" sh -c 'cp -a /var/lib/postgresql/a356-backup-good/. /var/lib/postgresql/a356-backup/'
docker exec -u 0 "$PGNAME" sh -c 'chmod u+w /var/lib/postgresql/a356-backup/manifest.json && dd if=/dev/zero of=/var/lib/postgresql/a356-backup/manifest.json bs=1 count=1 conv=notrunc'
phase restore-corrupt-manifest 4 restore "$OPERATOR"
copy_report restore-corrupt-manifest
grep -q '"replay_ready": false' "$EVIDENCE/live/restore-corrupt-manifest-readiness.json"
grep -q '"reason": "manifest_hash_mismatch"' "$EVIDENCE/live/restore-corrupt-manifest-readiness.json"

docker exec -u 0 "$PGNAME" sh -c 'cp -a /var/lib/postgresql/a356-backup-good/. /var/lib/postgresql/a356-backup/'
phase restore 0 restore "$OPERATOR"
copy_report restore
grep -q '"replay_ready": true' "$EVIDENCE/live/restore-readiness.json"
grep -q '"all_state_certified": false' "$EVIDENCE/live/restore-readiness.json"
grep -q '"w04_gate": "open"' "$EVIDENCE/live/restore-readiness.json"
grep -q '"parent_acceptance": false' "$EVIDENCE/live/restore-readiness.json"
grep -q '"production_ops": false' "$EVIDENCE/live/restore-readiness.json"
grep -q '"production_slo_certified": false' "$EVIDENCE/live/restore-readiness.json"
grep -q '"pitr_ready": false' "$EVIDENCE/live/restore-readiness.json"
grep -q '"offset_min_used": false' "$EVIDENCE/live/restore-readiness.json"
grep -q '"counts_ok": true' "$EVIDENCE/live/restore-readiness.json"
grep -q '"decimal_ok": true' "$EVIDENCE/live/restore-readiness.json"
grep -q '"null_ok": true' "$EVIDENCE/live/restore-readiness.json"
grep -q '"shop_gold_ok": true' "$EVIDENCE/live/restore-readiness.json"
grep -q '"checkpoint_copy_ok": true' "$EVIDENCE/live/restore-readiness.json"
grep -q '"key_reused": false' "$EVIDENCE/live/restore-readiness.json"
grep -q '"post_cut_absent_on_restore": true' "$EVIDENCE/live/restore-readiness.json"
grep -q '"post_cut_present_on_source": true' "$EVIDENCE/live/restore-readiness.json"
grep -q '"history_insufficient": false' "$EVIDENCE/live/restore-readiness.json"

docker exec -u 0 "$PGNAME" sh -c 'umask 077
  pass=$(tr -d "\r\n" < /secrets/admin.pass)
  printf "%s\n" "/var/run/postgresql:5432:*:oo_a356_admin:${pass}" > /tmp/a356.pgpass
  chmod 600 /tmp/a356.pgpass
  PGPASSFILE=/tmp/a356.pgpass PGHOST=/var/run/postgresql PGUSER=oo_a356_admin PGPORT=5432 \
    psql --no-password --no-psqlrc --tuples-only --no-align -d postgres \
    -c "SELECT datname FROM pg_database WHERE datname LIKE '\''oo_a356_%'\'' ORDER BY 1"
  status=$?
  rm -f /tmp/a356.pgpass
  exit $status' > "$EVIDENCE/live/databases.txt"

if grep -R -l -a -F -f "$ROOT/secrets/admin.pass" "$EVIDENCE" > "$ROOT/password-leak-files.txt"; then
  cp "$ROOT/password-leak-files.txt" "$EVIDENCE/live/password-leak-files.txt"
  echo "password leaked into evidence" >&2
  exit 1
fi
printf '%s\n' none > "$EVIDENCE/live/password-leak-files.txt"
if grep -R -l -a -F "PRIVATE KEY" "$EVIDENCE" > "$ROOT/private-key-files.txt"; then
  cp "$ROOT/private-key-files.txt" "$EVIDENCE/live/private-key-files.txt"
  echo "private key leaked into evidence" >&2
  exit 1
fi
printf '%s\n' none > "$EVIDENCE/live/private-key-files.txt"
printf '%s\n' 0 > "$EVIDENCE/live/run-all.exit"
echo "live restore finished"
