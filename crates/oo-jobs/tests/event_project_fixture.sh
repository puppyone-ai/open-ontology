#!/bin/bash
# Reference applicator for jobs-event-project/1. Fixed psql -f paths only.
# 0000 and 0001 stay the approved files. This script does not edit them.
set -euo pipefail
umask 077

EVIDENCE=${1:?evidence directory}
CONTAINER=${2:-oo-a075-pg1710-jobs}
UUID=eef831aa-dba4-4af2-a78b-541bc9e0a075
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$SCRIPT_DIR/.." && pwd)
REPO=$(cd "$CRATE/../.." && pwd)
SQL=/fixture/tests/fixtures/event_project
V1_CONTRACT=$REPO/contracts/jobs/control-schema/jobs-control-v1.json
CONTRACT=$REPO/contracts/jobs/event-project-binding/jobs-event-project-v1.json
ZERO=0000000000000000000000000000000000000000000000000000000000000000
BOOT_PIN=445d7248aa252a96a4103ae1b0be0000744eef2c0603037c19df6a0f8eeb0b52
EXPAND_PIN=b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75
V1_JSON_PIN=d352c86dbf2836360e541a3511af23674ce9692e687a6ac7041d99118dc8c2ca
EVENT_PIN=4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee

mkdir -p "$EVIDENCE"
exec > >(tee -a "$EVIDENCE/harness.log") 2>&1

owner=$(docker inspect --format '{{index .Config.Labels "oo.owner"}}' "$CONTAINER")
run_id=$(docker inspect --format '{{index .Config.Labels "oo.run_id"}}' "$CONTAINER")
test "$owner" = a075
test "$run_id" = "$UUID"

hash_file() {
  if [ -x /usr/bin/shasum ]; then
    /usr/bin/shasum -a 256 "$1" | awk '{print $1}'
  else
    /usr/bin/sha256sum "$1" | awk '{print $1}'
  fi
}

python_hash() {
  python3 - "$1" <<'PY'
import hashlib, pathlib, sys
print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())
PY
}

BOOT=$(hash_file "$CRATE/migrations/0000_bootstrap_roles.sql")
EXPAND=$(hash_file "$CRATE/migrations/0001_jobs_control_expand.sql")
EVENT=$(hash_file "$CRATE/migrations/0002_event_project_binding_expand.sql")
V1_JSON=$(hash_file "$V1_CONTRACT")
BOOT_PY=$(python_hash "$CRATE/migrations/0000_bootstrap_roles.sql")
EXPAND_PY=$(python_hash "$CRATE/migrations/0001_jobs_control_expand.sql")
EVENT_PY=$(python_hash "$CRATE/migrations/0002_event_project_binding_expand.sql")
test "$BOOT" = "$BOOT_PIN"
test "$EXPAND" = "$EXPAND_PIN"
test "$V1_JSON" = "$V1_JSON_PIN"
test "$BOOT" = "$BOOT_PY"
test "$EXPAND" = "$EXPAND_PY"
test "$EVENT" = "$EVENT_PY"
test "$EVENT" = "$EVENT_PIN"
python3 - "$CONTRACT" "$V1_CONTRACT" "$BOOT" "$EXPAND" "$EVENT" "$V1_JSON" <<'PY'
import json, sys
contract, v1_path, boot, expand, event, v1_json = sys.argv[1:]
doc = json.load(open(contract))
files = {item["filename"]: item for item in doc["files"]}
assert files["0000_bootstrap_roles.sql"]["sha256"] == boot
assert files["0001_jobs_control_expand.sql"]["sha256"] == expand
assert files["0002_event_project_binding_expand.sql"]["sha256"] == event
assert doc["schema_version"] == "jobs-event-project/1"
assert doc["migration_lock_key"] == 6401202601
assert doc["immutable_v1"]["contract_sha256"] == v1_json
v1 = json.load(open(v1_path))
assert v1["schema_version"] == "jobs-control/1"
assert v1["files"][0]["sha256"] == boot
assert v1["files"][1]["sha256"] == expand
print("contract_checksums=match")
PY
printf '%s\n' "$BOOT" > "$EVIDENCE/bootstrap.sha256"
printf '%s\n' "$EXPAND" > "$EVIDENCE/expand.sha256"
printf '%s\n' "$EVENT" > "$EVIDENCE/event_project.sha256"
printf '%s\n' "$V1_JSON" > "$EVIDENCE/jobs-control-v1.sha256"

psql_admin() {
  local db=$1
  shift
  docker exec "$CONTAINER" psql -U oo_fixture_admin -d "$db" -X -v ON_ERROR_STOP=1 "$@"
}

psql_login() {
  local db=$1
  shift
  docker exec "$CONTAINER" psql -U oo_a075_login -d "$db" -X -v ON_ERROR_STOP=1 "$@"
}

psql_owner() {
  local db=$1
  shift
  docker exec "$CONTAINER" psql -U oo_a075_owner_login -d "$db" -X -v ON_ERROR_STOP=1 "$@"
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

require_text() {
  local name=$1
  local text=$2
  if ! grep -F "$text" "$EVIDENCE/$name.stderr" >/dev/null; then
    echo "FAIL $name missing text: $text"
    exit 1
  fi
}

require_line() {
  local name=$1
  local line=$2
  if ! grep -qx "$line" "$EVIDENCE/$name.stdout"; then
    echo "FAIL $name missing line: $line"
    exit 1
  fi
}

reset_cluster() {
  run_ok reset psql_admin postgres -f "$SQL/reset_cluster.sql"
}

install_v1() {
  local db=$1
  local tag=$2
  run_ok "${tag}_create_db" psql_admin postgres -c "CREATE DATABASE ${db}"
  run_ok "${tag}_v1" psql_admin "$db" \
    -v "checksum_bootstrap=$BOOT" \
    -v "checksum_expand=$EXPAND" \
    -f /fixture/tests/fixtures/protocol_first.psql
  run_ok "${tag}_members" psql_admin "$db" -f "$SQL/create_members.sql"
  run_ok "${tag}_whoami" psql_login "$db" -At -f "$SQL/whoami_login.sql"
  require_line "${tag}_whoami" 'oo_a075_login|oo_a075_login'
}

behavior() {
  local db=$1
  local tag=$2
  run_ok "${tag}_prepare" psql_login "$db" -f "$SQL/prepare_behavior.sql"
  run_ok "${tag}_transitions" psql_login "$db" -At -f "$SQL/transitions.sql"
  require_line "${tag}_transitions" '3|true'
  run_ok "${tag}_same" psql_login "$db" -At -f "$SQL/insert_same_project.sql"
  require_line "${tag}_same" '0||99|spoofed-worker|true'
  run_ok "${tag}_acl" psql_admin "$db" -f "$SQL/assert_acl.sql"
  run_ok "${tag}_binding" psql_admin "$db" -f "$SQL/assert_binding.sql"
  run_ok "${tag}_before_neg" psql_admin "$db" -At -F '|' -f "$SQL/snapshot_data.sql"

  run_fail "${tag}_missing" 23503 psql_login "$db" -v VERBOSITY=verbose -f "$SQL/insert_missing_task.sql"
  require_text "${tag}_missing" 'task_events_task_id_fkey'
  run_fail "${tag}_cross" 23503 psql_login "$db" -v VERBOSITY=verbose -f "$SQL/insert_cross_project.sql"
  require_text "${tag}_cross" 'task_events_task_project_fkey'
  run_fail "${tag}_reassign_event" 42501 psql_login "$db" -v VERBOSITY=verbose -f "$SQL/reassign_event.sql"
  run_fail "${tag}_reassign_task" 42501 psql_login "$db" -v VERBOSITY=verbose -f "$SQL/reassign_task_project.sql"
  run_fail "${tag}_drop" 42501 psql_login "$db" -v VERBOSITY=verbose -f "$SQL/drop_constraint.sql"
  run_fail "${tag}_disable" 42501 psql_login "$db" -v VERBOSITY=verbose -f "$SQL/disable_trigger.sql"
  run_fail "${tag}_replica" 42501 psql_login "$db" -v VERBOSITY=verbose -f "$SQL/set_replication_role.sql"
  run_fail "${tag}_set_owner" 42501 psql_login "$db" -v VERBOSITY=verbose -f "$SQL/set_role_owner.sql"
  run_fail "${tag}_owner_reassign" 23514 psql_owner "$db" -v VERBOSITY=verbose -f "$SQL/owner_reassign_event.sql"
  require_text "${tag}_owner_reassign" 'task event is immutable'
  run_fail "${tag}_fk_project" 23503 psql_admin "$db" -v VERBOSITY=verbose -f "$SQL/fk_blocks_project_change.sql"
  require_text "${tag}_fk_project" 'task_events_task_project_fkey'

  run_ok "${tag}_after_neg" psql_admin "$db" -At -F '|' -f "$SQL/snapshot_data.sql"
  diff -u "$EVIDENCE/${tag}_before_neg.stdout" "$EVIDENCE/${tag}_after_neg.stdout"
  run_ok "${tag}_project" psql_admin "$db" -At \
    -v task_id='a0750001-0001-4000-8000-000000000012' \
    -f "$SQL/project_of.sql"
  require_line "${tag}_project" '11111111-1111-4111-8111-111111111111'
  run_ok "${tag}_note_missing" psql_admin "$db" -At -v note='missing task' -f "$SQL/count_note.sql"
  require_line "${tag}_note_missing" '0'
  run_ok "${tag}_note_cross" psql_admin "$db" -At -v note='cross-project' -f "$SQL/count_note.sql"
  require_line "${tag}_note_cross" '0'
  run_ok "${tag}_trigger_on" psql_admin "$db" -At -f "$SQL/trigger_enabled.sql"
  require_line "${tag}_trigger_on" 'tasks_enforce_update|O'

  run_ok "${tag}_owner_drop" psql_owner "$db" -f "$SQL/owner_drop_rollback.sql"
  run_ok "${tag}_oid_after_owner" psql_admin "$db" -At -f "$SQL/constraint_oid.sql"
  test -s "$EVIDENCE/${tag}_oid_after_owner.stdout"

  set +e
  psql_login "$db" -f "$SQL/hold_heartbeat.sql" \
    >"$EVIDENCE/${tag}_hold_heartbeat.stdout" 2>"$EVIDENCE/${tag}_hold_heartbeat.stderr" &
  local heartbeat_pid=$!
  set -e
  sleep 1
  set +e
  psql_login "$db" -f "$SQL/insert_same_heartbeat_wait.sql" \
    >"$EVIDENCE/${tag}_heartbeat_legal.stdout" 2>"$EVIDENCE/${tag}_heartbeat_legal.stderr" &
  local heartbeat_legal_pid=$!
  psql_login "$db" -v VERBOSITY=verbose -f "$SQL/insert_cross_heartbeat_wait.sql" \
    >"$EVIDENCE/${tag}_heartbeat_cross.stdout" 2>"$EVIDENCE/${tag}_heartbeat_cross.stderr" &
  local heartbeat_cross_pid=$!
  wait "$heartbeat_pid"
  local heartbeat_rc=$?
  wait "$heartbeat_legal_pid"
  local heartbeat_legal_rc=$?
  wait "$heartbeat_cross_pid"
  local heartbeat_cross_rc=$?
  set -e
  printf '%s\n' "$heartbeat_rc" > "$EVIDENCE/${tag}_hold_heartbeat.exit"
  printf '%s\n' "$heartbeat_legal_rc" > "$EVIDENCE/${tag}_heartbeat_legal.exit"
  printf '%s\n' "$heartbeat_cross_rc" > "$EVIDENCE/${tag}_heartbeat_cross.exit"
  test "$heartbeat_rc" -eq 0
  test "$heartbeat_legal_rc" -eq 0
  test "$heartbeat_cross_rc" -ne 0
  grep -E 'ERROR:  23503:|SQLSTATE: 23503' "$EVIDENCE/${tag}_heartbeat_cross.stderr" >/dev/null
  grep -F 'task_events_task_project_fkey' "$EVIDENCE/${tag}_heartbeat_cross.stderr" >/dev/null
  run_ok "${tag}_heartbeat_project" psql_admin "$db" -At \
    -v task_id='a0750001-0001-4000-8000-000000000014' \
    -f "$SQL/project_of.sql"
  require_line "${tag}_heartbeat_project" '11111111-1111-4111-8111-111111111111'
  run_ok "${tag}_heartbeat_count" psql_admin "$db" -At \
    -v note='concurrent-heartbeat-cross' -f "$SQL/count_note.sql"
  require_line "${tag}_heartbeat_count" '0'

  set +e
  psql_owner "$db" -v VERBOSITY=verbose -f "$SQL/hold_project_change.sql" \
    >"$EVIDENCE/${tag}_hold_project.stdout" 2>"$EVIDENCE/${tag}_hold_project.stderr" &
  local project_pid=$!
  set -e
  sleep 1
  set +e
  psql_login "$db" -f "$SQL/insert_same_wait.sql" \
    >"$EVIDENCE/${tag}_project_legal.stdout" 2>"$EVIDENCE/${tag}_project_legal.stderr" &
  local project_legal_pid=$!
  psql_login "$db" -v VERBOSITY=verbose -f "$SQL/insert_cross_wait.sql" \
    >"$EVIDENCE/${tag}_project_cross.stdout" 2>"$EVIDENCE/${tag}_project_cross.stderr" &
  local project_cross_pid=$!
  wait "$project_pid"
  local project_rc=$?
  wait "$project_legal_pid"
  local project_legal_rc=$?
  wait "$project_cross_pid"
  local project_cross_rc=$?
  set -e
  printf '%s\n' "$project_rc" > "$EVIDENCE/${tag}_hold_project.exit"
  printf '%s\n' "$project_legal_rc" > "$EVIDENCE/${tag}_project_legal.exit"
  printf '%s\n' "$project_cross_rc" > "$EVIDENCE/${tag}_project_cross.exit"
  test "$project_rc" -ne 0
  test "$project_legal_rc" -eq 0
  test "$project_cross_rc" -ne 0
  grep -E 'ERROR:  23514:|SQLSTATE: 23514' "$EVIDENCE/${tag}_hold_project.stderr" >/dev/null
  grep -E 'ERROR:  23503:|SQLSTATE: 23503' "$EVIDENCE/${tag}_project_cross.stderr" >/dev/null
  grep -F 'task_events_task_project_fkey' "$EVIDENCE/${tag}_project_cross.stderr" >/dev/null
  run_ok "${tag}_project_after" psql_admin "$db" -At \
    -v task_id='a0750001-0001-4000-8000-000000000012' \
    -f "$SQL/project_of.sql"
  require_line "${tag}_project_after" '11111111-1111-4111-8111-111111111111'
  run_ok "${tag}_project_cross_count" psql_admin "$db" -At \
    -v note='concurrent-cross' -f "$SQL/count_note.sql"
  require_line "${tag}_project_cross_count" '0'
  echo "behavior=${tag}"
}

echo "container=$CONTAINER boot=$BOOT expand=$EXPAND event=$EVENT"

reset_cluster

install_v1 oo_control_bad bad
run_ok bad_seed psql_login oo_control_bad -f "$SQL/seed_bad.sql"
run_ok bad_before psql_admin oo_control_bad -At -F '|' -f "$SQL/snapshot_data.sql"
run_ok bad_versions_before psql_admin oo_control_bad -At -F '|' -f "$SQL/snapshot_versions.sql"
run_fail bad_upgrade OO004 psql_admin oo_control_bad -v VERBOSITY=verbose \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/protocol_upgrade.psql"
require_text bad_upgrade 'refusing event project binding: 1 preexisting task_events row(s)'
require_text bad_upgrade 'does not rewrite, delete, or forge a correction'
run_ok bad_after psql_admin oo_control_bad -At -F '|' -f "$SQL/snapshot_data.sql"
run_ok bad_versions_after psql_admin oo_control_bad -At -F '|' -f "$SQL/snapshot_versions.sql"
diff -u "$EVIDENCE/bad_before.stdout" "$EVIDENCE/bad_after.stdout"
diff -u "$EVIDENCE/bad_versions_before.stdout" "$EVIDENCE/bad_versions_after.stdout"
run_ok bad_absent psql_admin oo_control_bad -At -f "$SQL/version_absent.sql"
require_line bad_absent 'true|true|0|0|0'
echo "bad_history_refused=rollback"

reset_cluster

install_v1 oo_control_rollback rollback
run_ok rollback_seed psql_login oo_control_rollback -f "$SQL/seed_rollback.sql"
run_ok rollback_before psql_admin oo_control_rollback -At -F '|' -f "$SQL/snapshot_data.sql"
run_fail rollback_apply OO003 psql_admin oo_control_rollback -v VERBOSITY=verbose \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/protocol_rollback.psql"
run_ok rollback_after psql_admin oo_control_rollback -At -F '|' -f "$SQL/snapshot_data.sql"
diff -u "$EVIDENCE/rollback_before.stdout" "$EVIDENCE/rollback_after.stdout"
run_ok rollback_absent psql_admin oo_control_rollback -At -f "$SQL/version_absent.sql"
require_line rollback_absent 'true|true|0|0|0'
echo "forced_follow_on_rolled_back"

reset_cluster

install_v1 oo_control_upgrade upgrade
run_ok upgrade_seed psql_login oo_control_upgrade -f "$SQL/seed_legal.sql"
run_ok upgrade_before psql_admin oo_control_upgrade -At -F '|' -f "$SQL/snapshot_data.sql"
run_ok upgrade_versions_before psql_admin oo_control_upgrade -At -F '|' -f "$SQL/snapshot_versions.sql"
run_fail upgrade_repeat_early OO002 psql_admin oo_control_upgrade -v VERBOSITY=verbose \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/protocol_repeat.psql"
run_ok upgrade_after_early psql_admin oo_control_upgrade -At -F '|' -f "$SQL/snapshot_data.sql"
diff -u "$EVIDENCE/upgrade_before.stdout" "$EVIDENCE/upgrade_after_early.stdout"
run_ok upgrade_apply psql_admin oo_control_upgrade \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/protocol_upgrade.psql"
run_ok upgrade_after psql_admin oo_control_upgrade -At -F '|' -f "$SQL/snapshot_data.sql"
run_ok upgrade_versions_after psql_admin oo_control_upgrade -At -F '|' -f "$SQL/snapshot_versions.sql"
diff -u "$EVIDENCE/upgrade_before.stdout" "$EVIDENCE/upgrade_after.stdout"
diff -u "$EVIDENCE/upgrade_versions_before.stdout" "$EVIDENCE/upgrade_versions_after.stdout"
run_ok upgrade_ready psql_admin oo_control_upgrade -At \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/ready_probe.sql"
tail -n 1 "$EVIDENCE/upgrade_ready.stdout" | grep -qx 't'
behavior oo_control_upgrade upgrade
echo "good_upgrade_preserved"

reset_cluster

run_ok fresh_create_db psql_admin postgres -c 'CREATE DATABASE oo_control'
run_ok fresh_apply psql_admin oo_control \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/protocol_fresh.psql"
run_ok fresh_members psql_admin oo_control -f "$SQL/create_members.sql"
run_ok fresh_whoami psql_login oo_control -At -f "$SQL/whoami_login.sql"
require_line fresh_whoami 'oo_a075_login|oo_a075_login'
run_ok fresh_binding psql_admin oo_control -f "$SQL/assert_binding.sql"
run_ok fresh_oid_before psql_admin oo_control -At -f "$SQL/constraint_oid.sql"
run_ok fresh_rows_before psql_admin oo_control -At -F '|' -f "$SQL/snapshot_migrations.sql"
run_ok fresh_repeat psql_admin oo_control \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/protocol_repeat.psql"
run_ok fresh_oid_after psql_admin oo_control -At -f "$SQL/constraint_oid.sql"
run_ok fresh_rows_after psql_admin oo_control -At -F '|' -f "$SQL/snapshot_migrations.sql"
diff -u "$EVIDENCE/fresh_oid_before.stdout" "$EVIDENCE/fresh_oid_after.stdout"
diff -u "$EVIDENCE/fresh_rows_before.stdout" "$EVIDENCE/fresh_rows_after.stdout"

run_fail fresh_bad_event_checksum OO001 psql_admin oo_control -v VERBOSITY=verbose \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$ZERO" \
  -f "$SQL/protocol_repeat.psql"
run_ok fresh_rows_after_bad_event psql_admin oo_control -At -F '|' -f "$SQL/snapshot_migrations.sql"
diff -u "$EVIDENCE/fresh_rows_before.stdout" "$EVIDENCE/fresh_rows_after_bad_event.stdout"

run_fail fresh_bad_expand_checksum OO001 psql_admin oo_control -v VERBOSITY=verbose \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$ZERO" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/protocol_repeat.psql"
run_ok fresh_rows_after_bad_expand psql_admin oo_control -At -F '|' -f "$SQL/snapshot_migrations.sql"
diff -u "$EVIDENCE/fresh_rows_before.stdout" "$EVIDENCE/fresh_rows_after_bad_expand.stdout"

run_ok fresh_ready psql_admin oo_control -At \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/ready_probe.sql"
tail -n 1 "$EVIDENCE/fresh_ready.stdout" | grep -qx 't'
run_ok fresh_ready_mismatch psql_admin oo_control -At \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$ZERO" \
  -f "$SQL/ready_probe.sql"
tail -n 1 "$EVIDENCE/fresh_ready_mismatch.stdout" | grep -qx 'f'
run_ok fresh_control_ready psql_admin oo_control -At \
  -v "checksum_expand=$EXPAND" \
  -f /fixture/tests/fixtures/ready_probe.sql
tail -n 1 "$EVIDENCE/fresh_control_ready.stdout" | grep -qx 't'

run_fail fresh_runtime_migrate 42501 psql_login oo_control -v VERBOSITY=verbose \
  -f "$SQL/runtime_reapply.psql"
run_ok fresh_oid_after_runtime psql_admin oo_control -At -f "$SQL/constraint_oid.sql"
diff -u "$EVIDENCE/fresh_oid_before.stdout" "$EVIDENCE/fresh_oid_after_runtime.stdout"

set +e
psql_admin oo_control -f "$SQL/hold_migration_lock.sql" \
  >"$EVIDENCE/fresh_hold_lock.stdout" 2>"$EVIDENCE/fresh_hold_lock.stderr" &
hold_pid=$!
set -e
sleep 1
run_fail fresh_lock_timeout 55P03 psql_admin oo_control -v VERBOSITY=verbose \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/protocol_repeat.psql"
set +e
wait "$hold_pid"
hold_rc=$?
set -e
printf '%s\n' "$hold_rc" > "$EVIDENCE/fresh_hold_lock.exit"
test "$hold_rc" -eq 0
run_ok fresh_repeat_after_lock psql_admin oo_control \
  -v "checksum_bootstrap=$BOOT" \
  -v "checksum_expand=$EXPAND" \
  -v "checksum_event_project=$EVENT" \
  -f "$SQL/protocol_repeat.psql"
run_ok fresh_rows_after_lock psql_admin oo_control -At -F '|' -f "$SQL/snapshot_migrations.sql"
diff -u "$EVIDENCE/fresh_rows_before.stdout" "$EVIDENCE/fresh_rows_after_lock.stdout"

behavior oo_control fresh
echo "fixture_result=pass"
