#!/bin/bash
# Reference applicator for jobs-control/1. Fixed psql -f paths only.
set -euo pipefail
umask 077

EVIDENCE=${1:?evidence directory}
CONTAINER=${2:-oo-a064-pg1710-jobs}
UUID=eef831aa-dba4-4af2-a78b-541bc9e0a064
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO=$(cd "$ROOT/../.." && pwd)
CONTRACT=$REPO/contracts/jobs/control-schema/jobs-control-v1.json
ZERO=0000000000000000000000000000000000000000000000000000000000000000

mkdir -p "$EVIDENCE"
exec > >(tee -a "$EVIDENCE/harness.log") 2>&1

owner=$(docker inspect --format '{{index .Config.Labels "oo.owner"}}' "$CONTAINER")
run_id=$(docker inspect --format '{{index .Config.Labels "oo.run_id"}}' "$CONTAINER")
test "$owner" = a064
test "$run_id" = "$UUID"

hash_file() {
  if [ -x /usr/bin/shasum ]; then
    /usr/bin/shasum -a 256 "$1" | awk '{print $1}'
  else
    /usr/bin/sha256sum "$1" | awk '{print $1}'
  fi
}

is_hex64() {
  [[ "$1" =~ ^[0-9a-f]{64}$ ]]
}

BOOT=$(hash_file "$ROOT/migrations/0000_bootstrap_roles.sql")
EXPAND=$(hash_file "$ROOT/migrations/0001_jobs_control_expand.sql")
is_hex64 "$BOOT"
is_hex64 "$EXPAND"
python3 - "$CONTRACT" "$BOOT" "$EXPAND" <<'PY'
import json, sys
path, boot, expand = sys.argv[1:]
doc = json.load(open(path))
files = {item["filename"]: item for item in doc["files"]}
assert files["0000_bootstrap_roles.sql"]["sha256"] == boot
assert files["0001_jobs_control_expand.sql"]["sha256"] == expand
assert doc["schema_version"] == "jobs-control/1"
assert doc["migration_lock_key"] == 6401202601
print("contract_checksums=match")
PY
printf '%s\n' "$BOOT" > "$EVIDENCE/bootstrap.sha256"
printf '%s\n' "$EXPAND" > "$EVIDENCE/expand.sha256"

psql() {
  docker exec "$CONTAINER" psql -U oo_fixture_admin -X -v ON_ERROR_STOP=1 "$@"
}

run_ok() {
  local name=$1
  shift
  set +e
  "$@" >"$EVIDENCE/$name.stdout" 2>"$EVIDENCE/$name.stderr"
  local rc=$?
  set -e
  printf '%s\n' "$rc" > "$EVIDENCE/$name.exit"
  if [ "$rc" -ne 0 ]; then
    echo "FAIL $name exit=$rc"
    exit "$rc"
  fi
  echo "ok $name"
}

run_fail() {
  local name=$1
  local expected=$2
  shift 2
  set +e
  "$@" >"$EVIDENCE/$name.stdout" 2>"$EVIDENCE/$name.stderr"
  local rc=$?
  set -e
  printf '%s\n' "$rc" > "$EVIDENCE/$name.exit"
  if [ "$rc" -eq 0 ]; then
    echo "FAIL $name unexpectedly succeeded"
    exit 1
  fi
  if ! grep -E "ERROR:  ${expected}:|SQLSTATE: ${expected}" "$EVIDENCE/$name.stderr" >/dev/null; then
    echo "FAIL $name expected SQLSTATE $expected"
    exit 1
  fi
  echo "rejected $name $expected"
}

echo "container=$CONTAINER boot=$BOOT expand=$EXPAND"

run_ok whoami psql -d postgres -At -f /fixture/tests/fixtures/whoami.sql
run_ok reset psql -d postgres -f /fixture/tests/fixtures/reset_databases.sql
run_ok protocol_first psql -d oo_control \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -f /fixture/tests/fixtures/protocol_first.psql
run_ok count_before psql -d oo_control -At -f /fixture/tests/fixtures/relation_count.sql
run_ok rows_before psql -d oo_control -At -f /fixture/tests/fixtures/migration_rows.sql
run_ok protocol_repeat psql -d oo_control \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -f /fixture/tests/fixtures/protocol_repeat.psql
run_ok count_after_repeat psql -d oo_control -At -f /fixture/tests/fixtures/relation_count.sql
run_ok rows_after_repeat psql -d oo_control -At -f /fixture/tests/fixtures/migration_rows.sql
diff -u "$EVIDENCE/count_before.stdout" "$EVIDENCE/count_after_repeat.stdout"
diff -u "$EVIDENCE/rows_before.stdout" "$EVIDENCE/rows_after_repeat.stdout"
echo "repeat_application=no_duplicate"

run_fail checksum_mismatch OO001 psql -d oo_control -v VERBOSITY=verbose \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$ZERO" \
  -f /fixture/tests/fixtures/protocol_repeat.psql
run_ok rows_after_mismatch psql -d oo_control -At -f /fixture/tests/fixtures/migration_rows.sql
diff -u "$EVIDENCE/rows_before.stdout" "$EVIDENCE/rows_after_mismatch.stdout"

run_ok ready_match psql -d oo_control -At \
  -v "checksum_expand=$EXPAND" \
  -f /fixture/tests/fixtures/ready_probe.sql
tail -n 1 "$EVIDENCE/ready_match.stdout" | grep -qx 't'
run_ok ready_mismatch psql -d oo_control -At \
  -v "checksum_expand=$ZERO" \
  -f /fixture/tests/fixtures/ready_probe.sql
tail -n 1 "$EVIDENCE/ready_mismatch.stdout" | grep -qx 'f'
echo "readiness_mismatch=false"

run_ok privileges psql -d oo_control -f /fixture/tests/fixtures/assert_privileges.sql
run_ok other_namespace psql -d oo_control -f /fixture/tests/fixtures/prepare_other_namespace.sql
run_ok legal psql -d oo_control -f /fixture/tests/fixtures/legal_flow.sql
run_ok snapshot_before psql -d oo_control -At -F '|' -f /fixture/tests/fixtures/snapshot.sql

while IFS=$'\t' read -r name file sqlstate; do
  [ "$name" = "name" ] && continue
  case "$file" in
    tests/fixtures/negatives/*.sql) ;;
    *) echo "bad fixture path $file"; exit 2 ;;
  esac
  run_fail "neg-$name" "$sqlstate" psql -d oo_control -v VERBOSITY=verbose -f "/fixture/$file"
done < "$ROOT/tests/fixtures/negatives.tsv"

run_ok drop_nobody psql -d oo_control -f /fixture/tests/fixtures/drop_nobody.sql
run_ok snapshot_after psql -d oo_control -At -F '|' -f /fixture/tests/fixtures/snapshot.sql
diff -u "$EVIDENCE/snapshot_before.stdout" "$EVIDENCE/snapshot_after.stdout"
echo "negatives_left_state_unchanged"

run_ok audit_gap psql -d oo_control -f /fixture/tests/fixtures/audit_insert_gap.sql
run_ok audit_gap_probe psql -d oo_control -At -f /fixture/tests/fixtures/audit_insert_gap_probe.sql
grep -qx '1' "$EVIDENCE/audit_gap_probe.stdout"
echo "audit_direct_insert=allowed"

set +e
psql -d oo_control -f /fixture/tests/fixtures/hold_migration_lock.sql \
  >"$EVIDENCE/hold_lock.stdout" 2>"$EVIDENCE/hold_lock.stderr" &
hold_pid=$!
set -e
sleep 2
run_fail lock_timeout 55P03 psql -d oo_control -v VERBOSITY=verbose \
  -f /fixture/tests/fixtures/try_migration_lock.sql
set +e
wait "$hold_pid"
hold_rc=$?
set -e
printf '%s\n' "$hold_rc" > "$EVIDENCE/hold_lock.exit"
if [ "$hold_rc" -ne 0 ]; then
  echo "FAIL hold_lock exit=$hold_rc"
  exit "$hold_rc"
fi
echo "concurrent_lock=55P03"
run_ok protocol_after_lock psql -d oo_control \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -f /fixture/tests/fixtures/protocol_repeat.psql

run_fail failed_fresh OO003 psql -d oo_control_fail -v VERBOSITY=verbose \
  -f /fixture/tests/fixtures/failed_fresh.sql
run_ok failed_fresh_probe psql -d oo_control_fail -At -F '|' \
  -f /fixture/tests/fixtures/failed_fresh_probe.sql
grep -qx 't|t' "$EVIDENCE/failed_fresh_probe.stdout"

run_fail failed_follow OO003 psql -d oo_control -v VERBOSITY=verbose \
  -f /fixture/tests/fixtures/failed_follow_on.sql
run_ok failed_follow_probe psql -d oo_control -At \
  -f /fixture/tests/fixtures/failed_follow_probe.sql
grep -qx 't' "$EVIDENCE/failed_follow_probe.stdout"
grep -qx 'jobs-bootstrap/1' "$EVIDENCE/failed_follow_probe.stdout"
grep -qx 'jobs-control/1' "$EVIDENCE/failed_follow_probe.stdout"
if grep -q 'jobs-control/2' "$EVIDENCE/failed_follow_probe.stdout"; then
  echo "FAIL follow-on version was committed"
  exit 1
fi
echo "failed_migration_rolled_back"

run_ok archive_roles psql -d oo_control -At -F '|' -f /fixture/tests/fixtures/archive_roles.sql
run_ok archive_grants psql -d oo_control -At -F '|' -f /fixture/tests/fixtures/archive_grants.sql
echo "fixture_result=pass"
