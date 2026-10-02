#!/bin/bash
# Reference applicator and target-fence checks for target-fence-receipts/1.
# Real LOGIN sessions. Explicit SQLSTATE. Durable rows are compared after failures.
set -euo pipefail
umask 077

EVIDENCE=${1:?evidence directory}
CONTAINER=${2:?postgres container name}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO=$(cd "$ROOT/../.." && pwd)
CONTRACT=$REPO/contracts/pipeline/target-fence-receipts/target-fence-receipts-v1.json
FX=/var/lib/oo-fixture/tests/fixtures
ZERO=0000000000000000000000000000000000000000000000000000000000000000
OWNER_A=11111111-1111-4111-8111-111111111111
OWNER_B=22222222-2222-4222-8222-222222222222

mkdir -p "$EVIDENCE"
exec > >(tee -a "$EVIDENCE/harness.log") 2>&1

hash_file() {
  if [ -x /usr/bin/shasum ]; then
    /usr/bin/shasum -a 256 "$1" | awk '{print $1}'
  else
    /usr/bin/sha256sum "$1" | awk '{print $1}'
  fi
}

owner=$(docker inspect --format '{{index .Config.Labels "oo.owner"}}' "$CONTAINER")
kind=$(docker inspect --format '{{index .Config.Labels "oo.kind"}}' "$CONTAINER")
test "$owner" = a072
test "$kind" = postgres
privileged=$(docker inspect --format '{{.HostConfig.Privileged}}' "$CONTAINER")
test "$privileged" = false
if docker inspect --format '{{json .HostConfig.PortBindings}}' "$CONTAINER" | grep -q HostPort; then
  echo "host port is published" >&2
  exit 1
fi
docker inspect --format 'image={{.Config.Image}} privileged={{.HostConfig.Privileged}} capdrop={{json .HostConfig.CapDrop}} capadd={{json .HostConfig.CapAdd}} ports={{json .HostConfig.PortBindings}} tmpfs={{json .HostConfig.Tmpfs}} network={{.HostConfig.NetworkMode}}' \
  "$CONTAINER" > "$EVIDENCE/container-identity.txt"

BOOT=$(hash_file "$ROOT/migrations/0000_bootstrap_roles.sql")
EXPAND=$(hash_file "$ROOT/migrations/0001_target_fence_expand.sql")
python3 - "$CONTRACT" "$BOOT" "$EXPAND" <<'PY'
import json, sys
path, boot, expand = sys.argv[1:]
doc = json.load(open(path))
files = {item["filename"]: item for item in doc["files"]}
assert files["0000_bootstrap_roles.sql"]["sha256"] == boot
assert files["0001_target_fence_expand.sql"]["sha256"] == expand
assert doc["schema_version"] == "target-fence-receipts/1"
assert doc["migration_lock_key"] == 6404202605
print("contract_checksums=match")
PY
printf '%s\n' "$BOOT" > "$EVIDENCE/bootstrap.sha256"
printf '%s\n' "$EXPAND" > "$EVIDENCE/expand.sha256"

admin_psql() {
  docker exec -i "$CONTAINER" bash -s -- "$@" <<'EOS'
set -euo pipefail
export PGPASSWORD="$(cat /run/secrets/admin_password)"
exec psql -U oo_fixture_admin -h 127.0.0.1 -X -q -P pager=off -v ON_ERROR_STOP=1 -v VERBOSITY=verbose "$@"
EOS
}

login_psql() {
  docker exec -e "PGAPPNAME=${OO_PGAPPNAME:-a072-psql}" -i "$CONTAINER" bash -s -- "$@" <<'EOS'
set -euo pipefail
secret=$1
user=$2
shift 2
export PGPASSWORD="$(cat "/var/lib/oo-fixture/${secret}")"
exec psql -U "$user" -h 127.0.0.1 -X -q -P pager=off -v ON_ERROR_STOP=1 -v VERBOSITY=verbose "$@"
EOS
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
  if ! grep -E "ERROR:  ${expected}:|FATAL:  ${expected}:|SQLSTATE: ${expected}" "$EVIDENCE/$name.stderr" >/dev/null; then
    echo "FAIL $name expected SQLSTATE $expected"
    exit 1
  fi
  echo "rejected $name $expected"
}

# Connection refusal is returned before psql applies VERBOSITY, so the client
# line has no SQLSTATE. Authentication failure is logged with application_name
# [unknown], so the SQLSTATE record is the verbose server line for this user
# and database. log_line_prefix includes %u and %d.
enable_verbose_server_log() {
  admin_psql -d postgres -c "ALTER SYSTEM SET log_error_verbosity = 'verbose'" >/dev/null
  admin_psql -d postgres -c "ALTER SYSTEM SET log_line_prefix = '%m [%p] %u@%d '" >/dev/null
  admin_psql -d postgres -c "SELECT pg_reload_conf()" >/dev/null
}

run_fail_connect() {
  local name=$1
  local expected=$2
  local secret=$3
  local user=$4
  shift 4
  set +e
  login_psql "$secret" "$user" "$@" >"$EVIDENCE/$name.stdout" 2>"$EVIDENCE/$name.stderr"
  local rc=$?
  set -e
  printf '%s\n' "$rc" > "$EVIDENCE/$name.exit"
  if [ "$rc" -eq 0 ]; then
    echo "FAIL $name unexpectedly succeeded"
    exit 1
  fi
  local attempt line_ok=0
  : > "$EVIDENCE/$name.server.log"
  for attempt in $(seq 1 20); do
    if docker logs --since 20s "$CONTAINER" 2>&1 | grep -F "${user}@oo_agent" | grep -F "FATAL:  ${expected}:" > "$EVIDENCE/$name.server.log"; then
      line_ok=1
      break
    fi
    sleep 0.25
  done
  if [ "$line_ok" -ne 1 ]; then
    echo "FAIL $name server log had no SQLSTATE $expected"
    exit 1
  fi
  cat "$EVIDENCE/$name.server.log" >> "$EVIDENCE/$name.stderr"
  if ! grep -E "ERROR:  ${expected}:|FATAL:  ${expected}:|SQLSTATE: ${expected}" "$EVIDENCE/$name.stderr" >/dev/null; then
    echo "FAIL $name expected SQLSTATE $expected"
    exit 1
  fi
  echo "rejected $name $expected"
}

generate_passwords() {
  local secret_name
  for secret_name in bootstrap_password runtime_password gateway_password agent_password; do
    docker exec "$CONTAINER" bash -c "od -An -N 24 -tx1 /dev/urandom | tr -d '[:space:]' > /var/lib/oo-fixture/${secret_name}; chmod 644 /var/lib/oo-fixture/${secret_name}"
  done
}

expect_exact() {
  local name=$1
  local expected=$2
  if ! grep -qx "$expected" "$EVIDENCE/$name.stdout"; then
    echo "FAIL $name expected [$expected]"
    exit 1
  fi
}

poll_sleep() {
  local app=$1
  local i line
  for i in $(seq 1 50); do
    line=$(admin_psql -d oo_agent -v app_name="$app" -f "$FX/activity_sleep.sql" | tr -d '[:space:]')
    if [ "$line" = "1" ]; then
      return 0
    fi
    sleep 0.25
  done
  echo "timed out waiting for $app"
  return 1
}

start_bg() {
  local name=$1
  shift
  "$@" >"$EVIDENCE/$name.stdout" 2>"$EVIDENCE/$name.stderr" &
  printf '%s\n' "$!" > "$EVIDENCE/$name.pid"
}

wait_bg() {
  local name=$1
  local expect_zero=${2:-1}
  local pid rc
  pid=$(cat "$EVIDENCE/$name.pid")
  set +e
  wait "$pid"
  rc=$?
  set -e
  printf '%s\n' "$rc" > "$EVIDENCE/$name.exit"
  if [ "$expect_zero" = 1 ] && [ "$rc" -ne 0 ]; then
    echo "FAIL $name exit=$rc"
    exit "$rc"
  fi
  if [ "$expect_zero" = 0 ] && [ "$rc" -eq 0 ]; then
    echo "FAIL $name unexpectedly succeeded"
    exit 1
  fi
  echo "waited $name exit=$rc"
}

cleanup() {
  docker exec "$CONTAINER" rm -f \
    /var/lib/oo-fixture/bootstrap_password \
    /var/lib/oo-fixture/runtime_password \
    /var/lib/oo-fixture/gateway_password \
    /var/lib/oo-fixture/agent_password >/dev/null 2>&1 || true
  return 0
}
trap cleanup EXIT

echo "container=$CONTAINER boot=$BOOT expand=$EXPAND"
run_ok pg_version admin_psql -d postgres -At -c 'SELECT version();'

docker exec "$CONTAINER" rm -rf /var/lib/oo-fixture
docker exec "$CONTAINER" mkdir -p /var/lib/oo-fixture/tests
docker cp "$ROOT/migrations" "$CONTAINER:/var/lib/oo-fixture/migrations"
docker cp "$ROOT/tests/fixtures" "$CONTAINER:/var/lib/oo-fixture/tests/fixtures"

if [ "${OO_FIXTURE_FROM:-full}" = "full" ]; then
reset_ok=0
for _try in 1 2 3 4 5; do
  if admin_psql -d postgres -f "$FX/reset_databases.sql" >"$EVIDENCE/reset.stdout" 2>"$EVIDENCE/reset.stderr"; then
    reset_ok=1
    break
  fi
  sleep 1
done
test "$reset_ok" = 1
echo "ok reset"

run_ok apply admin_psql -d oo_agent -f "$FX/apply_schema.psql"
run_fail bookkeeping_missing OO002 admin_psql -d oo_agent -v VERBOSITY=verbose \
  -v checksum_bootstrap="$BOOT" -v checksum_expand="$EXPAND" -f "$FX/protocol_repeat.psql"
run_ok ready_before admin_psql -d oo_agent -v checksum_expand="$EXPAND" -f "$FX/ready_function.sql"
expect_exact ready_before f
run_ok rows_missing admin_psql -d oo_agent -f "$FX/migration_rows.sql"
if grep -q . "$EVIDENCE/rows_missing.stdout"; then
  echo "FAIL bookkeeping rows existed before record"
  exit 1
fi

run_ok record admin_psql -d oo_agent \
  -v checksum_bootstrap="$BOOT" -v checksum_expand="$EXPAND" -f "$FX/record_migrations.psql"
run_ok protocol_repeat admin_psql -d oo_agent \
  -v checksum_bootstrap="$BOOT" -v checksum_expand="$EXPAND" -f "$FX/protocol_repeat.psql"
run_ok rows_ready admin_psql -d oo_agent -f "$FX/migration_rows.sql"
run_ok protocol_repeat_again admin_psql -d oo_agent \
  -v checksum_bootstrap="$BOOT" -v checksum_expand="$EXPAND" -f "$FX/protocol_repeat.psql"
run_ok rows_repeat admin_psql -d oo_agent -f "$FX/migration_rows.sql"
diff -u "$EVIDENCE/rows_ready.stdout" "$EVIDENCE/rows_repeat.stdout"
echo "repeat_application=no_duplicate"

run_fail checksum_mismatch OO001 admin_psql -d oo_agent -v VERBOSITY=verbose \
  -v checksum_bootstrap="$BOOT" -v checksum_expand="$ZERO" -f "$FX/protocol_repeat.psql"
run_ok rows_mismatch admin_psql -d oo_agent -f "$FX/migration_rows.sql"
diff -u "$EVIDENCE/rows_ready.stdout" "$EVIDENCE/rows_mismatch.stdout"
run_ok ready_match admin_psql -d oo_agent -v checksum_expand="$EXPAND" -f "$FX/ready_function.sql"
expect_exact ready_match t
run_ok ready_view admin_psql -d oo_agent -v checksum_expand="$EXPAND" -f "$FX/ready_view.sql"
expect_exact ready_view t
run_ok ready_mismatch admin_psql -d oo_agent -v checksum_expand="$ZERO" -f "$FX/ready_function.sql"
expect_exact ready_mismatch f
run_ok rows_after_ready admin_psql -d oo_agent -f "$FX/migration_rows.sql"
diff -u "$EVIDENCE/rows_ready.stdout" "$EVIDENCE/rows_after_ready.stdout"
echo "readiness_mismatch=false"

generate_passwords
run_ok logins admin_psql -d oo_agent -f "$FX/create_logins.sql"
run_ok synth admin_psql -d oo_agent -f "$FX/install_fixture_synth.sql"
run_ok privileges admin_psql -d oo_agent -f "$FX/assert_privileges.sql"
run_ok relations admin_psql -d oo_agent -f "$FX/relations.sql"
cat > "$EVIDENCE/relations.expected" <<'EOF'
apply_receipts|p|oo_target_owner
migration_lock|p|oo_target_owner
schema_migrations|p|oo_target_owner
target_generations|p|oo_target_owner
0
0
EOF
diff -u "$EVIDENCE/relations.expected" "$EVIDENCE/relations.stdout"
run_ok control_empty admin_psql -d oo_control -f "$FX/control_empty.sql"
expect_exact control_empty oo_control
grep -qx '0' "$EVIDENCE/control_empty.stdout"
run_ok bare_empty admin_psql -d oo_agent_bare -f "$FX/control_empty.sql"
expect_exact bare_empty oo_agent_bare
grep -qx '0' "$EVIDENCE/bare_empty.stdout"

run_ok whoami_runtime login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/whoami.sql"
expect_exact whoami_runtime 'oo_a072_runtime|oo_a072_runtime|f'
run_ok whoami_bootstrap login_psql bootstrap_password oo_a072_bootstrap -d oo_agent -f "$FX/whoami.sql"
expect_exact whoami_bootstrap 'oo_a072_bootstrap|oo_a072_bootstrap|f'
run_ok runtime_ready login_psql runtime_password oo_a072_runtime -d oo_agent \
  -v checksum_expand="$EXPAND" -f "$FX/runtime_ready.sql"
grep -qx 't' "$EVIDENCE/runtime_ready.stdout"
elif [ "${OO_FIXTURE_FROM}" = "connect" ]; then
  echo "resume=connect_without_reset"
  generate_passwords
  run_ok rotate_passwords admin_psql -d oo_agent -f "$FX/rotate_passwords.sql"
else
  echo "unknown OO_FIXTURE_FROM=${OO_FIXTURE_FROM}"
  exit 2
fi

enable_verbose_server_log
run_fail_connect gateway_connect 42501 gateway_password oo_a072_gateway -d oo_agent -c 'SELECT 1'
run_fail_connect agent_connect 42501 agent_password oo_a072_agent -d oo_agent -c 'SELECT 1'
run_fail runtime_set_config 42501 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_set_config.sql"
run_fail runtime_advisory 42501 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_advisory.sql"
run_fail runtime_insert 42501 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_insert.sql"
run_fail runtime_truncate 42501 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_truncate.sql"
run_fail runtime_temp 42501 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_temp.sql"
run_fail runtime_set_role 42501 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_set_role.sql"
run_fail runtime_create_function 42501 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_create_function.sql"
run_fail owner_insert OO014 login_psql bootstrap_password oo_a072_bootstrap -d oo_agent -f "$FX/owner_insert.sql"
run_fail owner_truncate OO014 login_psql bootstrap_password oo_a072_bootstrap -d oo_agent -f "$FX/owner_truncate.sql"
run_fail owner_update_migration OO014 login_psql bootstrap_password oo_a072_bootstrap -d oo_agent -f "$FX/owner_update_migration.sql"

run_fail closed_codec OO015 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_codec.sql"
run_fail closed_lineage OO015 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_lineage.sql"
run_fail closed_digest 23514 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_digest.sql"
run_fail closed_long 23514 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_long.sql"
run_fail closed_fraction 22003 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_fraction.sql"
run_fail closed_huge 22003 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_huge.sql"
run_fail closed_negative_pos 22003 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_negative_pos.sql"
run_fail closed_nul 22021 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_nul.sql"
run_fail closed_partial 23514 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_partial.sql"
run_fail closed_segment 23514 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_segment.sql"
run_fail negative_generation 22003 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_generation.sql"
run_fail zero_generation 22003 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/reject_generation_zero.sql"
run_ok receipts_before_flow admin_psql -d oo_agent -f "$FX/snapshot_receipts.sql"
if grep -q . "$EVIDENCE/receipts_before_flow.stdout"; then
  echo "FAIL closed metadata wrote a receipt"
  exit 1
fi
echo "closed_metadata_left_no_receipt"

run_ok clock_trap admin_psql -d oo_agent -f "$FX/create_clock_trap.sql"
run_ok fence_flow login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/fence_flow.sql"
run_ok drop_trap admin_psql -d oo_agent -f "$FX/drop_clock_trap.sql"
run_fail owner_delete_generation OO014 login_psql bootstrap_password oo_a072_bootstrap -d oo_agent -f "$FX/owner_delete_generation.sql"
run_ok shard_clock_after_delete admin_psql -d oo_agent -v shard=shard_clock -f "$FX/shard_line.sql"
expect_exact shard_clock_after_delete "1|adopted|$OWNER_A"
run_ok negative_key login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/negative_key.sql"
run_ok receipts_after_flow admin_psql -d oo_agent -f "$FX/snapshot_receipts.sql"
run_ok replay_takeover login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/replay_after_takeover.sql"
run_ok receipts_after_replay admin_psql -d oo_agent -f "$FX/snapshot_receipts.sql"
diff -u "$EVIDENCE/receipts_after_flow.stdout" "$EVIDENCE/receipts_after_replay.stdout"
run_ok shard_after_replay admin_psql -d oo_agent -v shard=shard_alpha -f "$FX/shard_line.sql"
expect_exact shard_after_replay "2|adopted|$OWNER_B"

run_fail conflict_payload OO010 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/conflict_payload.sql"
run_fail conflict_lineage OO010 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/conflict_lineage.sql"
run_fail conflict_codec OO010 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/conflict_codec.sql"
run_fail conflict_side OO010 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/conflict_side_rows.sql"
run_ok receipts_after_conflict admin_psql -d oo_agent -f "$FX/snapshot_receipts.sql"
diff -u "$EVIDENCE/receipts_after_replay.stdout" "$EVIDENCE/receipts_after_conflict.stdout"
run_ok shard_after_conflict admin_psql -d oo_agent -v shard=shard_alpha -f "$FX/shard_line.sql"
diff -u "$EVIDENCE/shard_after_replay.stdout" "$EVIDENCE/shard_after_conflict.stdout"
run_ok side_counts admin_psql -d oo_agent -f "$FX/side_counts.sql"
expect_exact side_counts '0|0'
run_fail owner_update_receipt OO014 login_psql bootstrap_password oo_a072_bootstrap -d oo_agent -f "$FX/owner_update_receipt.sql"
run_ok receipts_after_owner admin_psql -d oo_agent -f "$FX/snapshot_receipts.sql"
diff -u "$EVIDENCE/receipts_after_conflict.stdout" "$EVIDENCE/receipts_after_owner.stdout"
echo "conflicts_left_receipt_unchanged"

start_bg hold_excl login_psql runtime_password oo_a072_runtime -d oo_agent \
  -v app_name=a072-excl -v shard=shard_0 -v owner="$OWNER_A" -v sleep_seconds=8 \
  -f "$FX/hold_adopt.sql"
poll_sleep a072-excl
run_fail second_same_shard 55P03 login_psql runtime_password oo_a072_runtime -d oo_agent \
  -v shard=shard_0 -v owner="$OWNER_B" -f "$FX/second_adopt.sql"
wait_bg hold_excl 1
echo "same_shard_exclusive=55P03"

start_bg hold_left login_psql runtime_password oo_a072_runtime -d oo_agent \
  -v app_name=a072-left -v shard=shard_left -v owner="$OWNER_A" -v sleep_seconds=6 \
  -f "$FX/hold_adopt.sql"
start_bg hold_right login_psql runtime_password oo_a072_runtime -d oo_agent \
  -v app_name=a072-right -v shard=shard_right -v owner="$OWNER_A" -v sleep_seconds=6 \
  -f "$FX/hold_adopt.sql"
parallel_ok=0
for _try in $(seq 1 50); do
  left=$(admin_psql -d oo_agent -v app_name=a072-left -f "$FX/activity_sleep.sql" | tr -d '[:space:]')
  right=$(admin_psql -d oo_agent -v app_name=a072-right -f "$FX/activity_sleep.sql" | tr -d '[:space:]')
  if [ "$left" = 1 ] && [ "$right" = 1 ]; then
    parallel_ok=1
    break
  fi
  sleep 0.25
done
test "$parallel_ok" = 1
wait_bg hold_left 1
wait_bg hold_right 1
run_ok shard_left admin_psql -d oo_agent -v shard=shard_left -f "$FX/shard_line.sql"
run_ok shard_right admin_psql -d oo_agent -v shard=shard_right -f "$FX/shard_line.sql"
expect_exact shard_left "1|adopted|$OWNER_A"
expect_exact shard_right "1|adopted|$OWNER_A"
echo "distinct_shards_parallel=1"

start_bg hold_known login_psql runtime_password oo_a072_runtime -d oo_agent \
  -v app_name=a072-held -v shard=shard_held -v owner="$OWNER_A" -v sleep_seconds=8 \
  -f "$FX/hold_adopt.sql"
poll_sleep a072-held
run_fail other_connection OO011 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/other_connection.sql"
wait_bg hold_known 1
echo "other_connection_rejected=OO011"

run_fail same_backend_other_token OO017 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/oo017.sql"
run_ok shard_oo017 admin_psql -d oo_agent -v shard=shard_oo017 -f "$FX/shard_line.sql"
expect_exact shard_oo017 "1|adopted|$OWNER_A"
run_fail stale_generation OO013 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/stale_generation.sql"
run_ok counts_stale admin_psql -d oo_agent -v batch=batch-stale -f "$FX/counts_for.sql"
expect_exact counts_stale '0|0|0'
run_fail wrong_owner OO013 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/wrong_owner.sql"
run_ok counts_wrong_owner admin_psql -d oo_agent -v batch=batch-wrong-owner -f "$FX/counts_for.sql"
expect_exact counts_wrong_owner '0|0|0'

run_fail failed_commit OO018 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/fail_then_commit.sql"
run_ok shard_failed admin_psql -d oo_agent -v shard=shard_fail -f "$FX/shard_line.sql"
expect_exact shard_failed "1|failed|$OWNER_A"
run_ok fail_takeover login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/fail_takeover.sql"
expect_exact fail_takeover 2
run_fail rolled_back_fail OO011 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/fail_rollback.sql"
run_ok shard_rb admin_psql -d oo_agent -v shard=shard_rb -f "$FX/shard_line.sql"
expect_exact shard_rb "1|adopted|$OWNER_A"
run_ok rb_takeover login_psql runtime_password oo_a072_runtime -d oo_agent \
  -v shard=shard_rb -v owner="$OWNER_B" -f "$FX/second_adopt.sql"
grep -qx '2' "$EVIDENCE/rb_takeover.stdout"

start_bg hold_dead login_psql runtime_password oo_a072_runtime -d oo_agent \
  -v app_name=a072-dead -v shard=shard_dead -v owner="$OWNER_A" -v sleep_seconds=30 \
  -f "$FX/hold_adopt.sql"
poll_sleep a072-dead
run_ok terminate_dead admin_psql -d oo_agent -v app_name=a072-dead -f "$FX/terminate_app.sql"
wait_bg hold_dead 0
grep -E 'FATAL:  57P01:|SQLSTATE: 57P01' "$EVIDENCE/hold_dead.stderr" >/dev/null
run_ok takeover_dead login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/takeover_dead.sql"
run_ok counts_dead admin_psql -d oo_agent -v batch=batch-dead -f "$FX/counts_for.sql"
expect_exact counts_dead '0|0|1'
echo "terminated_backend_generation_advanced"

start_bg crash_before login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/crash_before.sql"
poll_sleep a072-crash-before
run_ok crash_before_open admin_psql -d oo_agent -v batch=batch-crash-before -f "$FX/counts_for.sql"
expect_exact crash_before_open '0|0|0'
run_ok terminate_before admin_psql -d oo_agent -v app_name=a072-crash-before -f "$FX/terminate_app.sql"
wait_bg crash_before 0
grep -E 'FATAL:  57P01:|SQLSTATE: 57P01' "$EVIDENCE/crash_before.stderr" >/dev/null
run_ok crash_before_after admin_psql -d oo_agent -v batch=batch-crash-before -f "$FX/counts_for.sql"
expect_exact crash_before_after '0|0|0'
echo "crash_before_commit_left_no_receipt"

start_bg crash_after login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/crash_after.sql"
crash_visible=0
for _try in $(seq 1 50); do
  sleeping=$(admin_psql -d oo_agent -v app_name=a072-crash-after -f "$FX/activity_sleep.sql" | tr -d '[:space:]')
  counts=$(admin_psql -d oo_agent -v batch=batch-crash-after -f "$FX/counts_for.sql" | tr -d '[:space:]')
  if [ "$sleeping" = 1 ] && [ "$counts" = "1|1|1" ]; then
    crash_visible=1
    break
  fi
  sleep 0.25
done
test "$crash_visible" = 1
run_ok terminate_after admin_psql -d oo_agent -v app_name=a072-crash-after -f "$FX/terminate_app.sql"
wait_bg crash_after 0
grep -E 'FATAL:  57P01:|SQLSTATE: 57P01' "$EVIDENCE/crash_after.stderr" >/dev/null
run_ok crash_after_durable admin_psql -d oo_agent -v batch=batch-crash-after -f "$FX/counts_for.sql"
expect_exact crash_after_durable '1|1|1'
echo "crash_after_commit_receipt_remains"

run_ok prepare_overflow login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/prepare_overflow.sql"
run_ok force_max admin_psql -d oo_agent -f "$FX/force_max.sql"
run_fail overflow_adopt 22003 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/overflow_adopt.sql"
run_ok check_overflow admin_psql -d oo_agent -f "$FX/check_overflow.sql"
expect_exact check_overflow '9223372036854775807|failed'

run_ok prepare_down login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/prepare_down.sql"
run_ok generations_before_key admin_psql -d oo_agent -f "$FX/snapshot_generations.sql"
run_fail decrease_generation 22003 admin_psql -d oo_agent -f "$FX/decrease_generation.sql"
run_ok check_down admin_psql -d oo_agent -f "$FX/check_down.sql"
expect_exact check_down 4
run_fail lock_key_change OO019 admin_psql -d oo_agent -f "$FX/lock_key_change.sql"
run_ok generations_after_key admin_psql -d oo_agent -f "$FX/snapshot_generations.sql"
python3 - "$EVIDENCE/generations_before_key.stdout" "$EVIDENCE/generations_after_key.stdout" <<'PY'
import pathlib, sys
before = pathlib.Path(sys.argv[1]).read_text().splitlines()
after = pathlib.Path(sys.argv[2]).read_text().splitlines()
old = next(line for line in before if line.startswith("shard_down|")).split("|")
new = next(line for line in after if line.startswith("shard_down|")).split("|")
assert new[1] == "4", new
assert new[2] == "adopted", new
assert new[4:] == old[4:], (old, new)
print("lock_keys_unchanged")
PY
run_fail runtime_force 42501 login_psql runtime_password oo_a072_runtime -d oo_agent -f "$FX/runtime_force_denied.sql"
run_ok drop_force admin_psql -d oo_agent -f "$FX/drop_force.sql"
grep -qx '0' "$EVIDENCE/drop_force.stdout"

start_bg hold_migration admin_psql -d oo_agent -f "$FX/hold_migration.sql"
poll_sleep a072-migration
run_fail migration_lock 55P03 admin_psql -d oo_agent -v VERBOSITY=verbose \
  -v checksum_bootstrap="$BOOT" -v checksum_expand="$EXPAND" -f "$FX/protocol_repeat.psql"
wait_bg hold_migration 1
run_ok protocol_after_lock admin_psql -d oo_agent \
  -v checksum_bootstrap="$BOOT" -v checksum_expand="$EXPAND" -f "$FX/protocol_repeat.psql"
echo "migration_executor_lock=55P03"

run_fail failed_fresh OO003 admin_psql -d oo_agent_fail -f "$FX/failed_fresh.sql"
run_ok failed_fresh_probe admin_psql -d oo_agent_fail -f "$FX/failed_fresh_probe.sql"
expect_exact failed_fresh_probe 't|t'
run_fail failed_follow OO003 admin_psql -d oo_agent -f "$FX/failed_follow_on.sql"
run_ok failed_follow_probe admin_psql -d oo_agent -f "$FX/failed_follow_probe.sql"
grep -qx 't' "$EVIDENCE/failed_follow_probe.stdout"
grep -qx 'target-bootstrap/1' "$EVIDENCE/failed_follow_probe.stdout"
grep -qx 'target-fence-receipts/1' "$EVIDENCE/failed_follow_probe.stdout"
if grep -q 'target-fence-receipts/2' "$EVIDENCE/failed_follow_probe.stdout"; then
  echo "FAIL follow-on version was committed"
  exit 1
fi
echo "failed_migration_rolled_back"

SCAN=$(mktemp -d /tmp/oo-a072-secret-scan.XXXXXX)
chmod 700 "$SCAN"
for secret_name in bootstrap_password runtime_password gateway_password agent_password; do
  docker cp "$CONTAINER:/var/lib/oo-fixture/$secret_name" "$SCAN/$secret_name"
done
python3 - "$EVIDENCE" "$SCAN" <<'PY'
import pathlib, sys
evidence, secret_dir = map(pathlib.Path, sys.argv[1:])
secrets = []
for name in ("bootstrap_password", "runtime_password", "gateway_password", "agent_password"):
    path = secret_dir / name
    if path.exists():
        secrets.append(path.read_text().strip())
if len(secrets) != 4 or any(len(secret) < 32 for secret in secrets):
    raise SystemExit("login passwords were not captured for the leak scan")
for path in evidence.rglob("*"):
    if not path.is_file():
        continue
    text = path.read_text(errors="ignore")
    for secret in secrets:
        if secret and secret in text:
            raise SystemExit(f"secret material in {path.name}")
print("secret_scan=clear")
PY
rm -rf "$SCAN"
echo "fixture_result=pass"
