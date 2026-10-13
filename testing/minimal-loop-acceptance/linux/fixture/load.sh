#!/bin/bash
# Load the public fixture into the source with one data session.
# The target stays empty. This load is operator prep, not MP-01b.
set -eu
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a407
REPO=/Users/redux/puppy-one/open-ontology
ROOT=${REPO}/evidence/${RUN_ID}/SP11/minimal-loop-acceptance
FIXTURE=${ROOT}/linux/focused-5/fixture
OUT=${ROOT}/fixture/load
SEC=/tmp/oo-a407-${RUN_ID}/secrets
mkdir -p "$OUT"
if [ ! -f "$FIXTURE/schema.sql" ] || [ ! -f "$FIXTURE/party.copy" ] || [ ! -f "$FIXTURE/entry.copy" ]; then
  echo "fixture files are absent" >&2
  printf '%s\n' 1 > "$OUT/load.exit"
  exit 1
fi

# The password stays in the container psql variable. The host argv does not carry it.
docker exec -u postgres -i oo-a407-src-pg bash -s <<'EOS'
set -eu
pw=$(cat /secrets/reader.pass)
psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_src -v pw="$pw" <<'SQL'
CREATE ROLE oo_a407_reader LOGIN PASSWORD :'pw';
SQL
EOS
docker exec -u postgres -i oo-a407-dst-pg bash -s <<'EOS'
set -eu
pw=$(cat /secrets/reader.pass)
psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_dst -v pw="$pw" <<'SQL'
CREATE ROLE oo_a407_reader LOGIN PASSWORD :'pw';
SQL
EOS

{
  printf '%s\n' '\set ON_ERROR_STOP on'
  printf '%s\n' "SET statement_timeout = '120s';"
  cat "$FIXTURE/schema.sql"
  printf '%s\n' "GRANT USAGE ON SCHEMA shop TO oo_a407_reader;"
  printf '%s\n' "GRANT SELECT ON shop.party, shop.account, shop.entry TO oo_a407_reader;"
  printf '%s\n' "COPY shop.party (party_key, display_name, active, phone, national_id) FROM STDIN WITH (FORMAT text);"
  cat "$FIXTURE/party.copy"
  printf '%s\n' '\.'
  printf '%s\n' "COPY shop.account (account_id, party_key, opened, label) FROM STDIN WITH (FORMAT text);"
  cat "$FIXTURE/account.copy"
  printf '%s\n' '\.'
  printf '%s\n' "COPY shop.entry (entry_id, account_id, qty, amount, memo) FROM STDIN WITH (FORMAT text);"
  cat "$FIXTURE/entry.copy"
  printf '%s\n' '\.'
  printf '%s\n' "\\pset tuples_only on"
  printf '%s\n' "\\pset format unaligned"
  printf '%s\n' "SELECT 'source_version=' || current_setting('server_version');"
  printf '%s\n' "SELECT 'source_system_identifier=' || system_identifier FROM pg_control_system();"
  printf '%s\n' "SELECT 'source_party=' || count(*) FROM shop.party;"
  printf '%s\n' "SELECT 'source_account=' || count(*) FROM shop.account;"
  printf '%s\n' "SELECT 'source_entry=' || count(*) FROM shop.entry;"
  printf '%s\n' "SELECT 'source_null_display=' || count(*) FROM shop.party WHERE display_name IS NULL;"
  printf '%s\n' "SELECT 'source_amount_3=' || amount::text FROM shop.entry WHERE entry_id = 3;"
  printf '%s\n' "SELECT 'source_qty_0=' || qty::text FROM shop.entry WHERE entry_id = 0;"
  printf '%s\n' "SELECT 'source_qty_1=' || qty::text FROM shop.entry WHERE entry_id = 1;"
  printf '%s\n' "SELECT 'source_qty_2=' || qty::text FROM shop.entry WHERE entry_id = 2;"
  printf '%s\n' "SELECT 'source_party_0_display=' || COALESCE(display_name, '\\N') FROM shop.party WHERE party_key = 'p00000';"
  printf '%s\n' "SELECT 'source_party_0_active=' || CASE WHEN active THEN 't' ELSE 'f' END FROM shop.party WHERE party_key = 'p00000';"
  printf '%s\n' "SELECT 'source_account_0_party=' || party_key FROM shop.account WHERE account_id = 0;"
  printf '%s\n' "SELECT 'source_activity=' || count(*) FROM pg_stat_activity WHERE datname = current_database() AND usename = current_user;"
  printf '%s\n' "SELECT 'source_ssl=' || current_setting('ssl');"
} | docker exec -i -u postgres oo-a407-src-pg psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_src > "$OUT/source-probe.stdout" 2> "$OUT/source-probe.stderr"
source_exit=$?
printf '%s\n' "$source_exit" > "$OUT/source-probe.exit"

{
  printf '%s\n' '\set ON_ERROR_STOP on'
  printf '%s\n' "\\pset tuples_only on"
  printf '%s\n' "\\pset format unaligned"
  printf '%s\n' "SELECT 'target_version=' || current_setting('server_version');"
  printf '%s\n' "SELECT 'target_system_identifier=' || system_identifier FROM pg_control_system();"
  printf '%s\n' "SELECT 'target_shop=' || count(*) FROM pg_namespace WHERE nspname = 'shop';"
  printf '%s\n' "SELECT 'target_entry_regclass=' || COALESCE(to_regclass('shop.entry')::text, 'absent');"
  printf '%s\n' "SELECT 'target_ssl=' || current_setting('ssl');"
} | docker exec -i -u postgres oo-a407-dst-pg psql -v ON_ERROR_STOP=1 -U oo_a407_admin -d oo_a407_dst > "$OUT/target-probe.stdout" 2> "$OUT/target-probe.stderr"
target_exit=$?
printf '%s\n' "$target_exit" > "$OUT/target-probe.exit"

activity=$(awk -F= '$1=="source_activity"{print $2}' "$OUT/source-probe.stdout" | tr -d '[:space:]')
{
  cat "$OUT/source-probe.stdout"
  cat "$OUT/target-probe.stdout"
  if [ "$activity" = "1" ]; then
    printf '%s\n' "loader_connections=1"
  else
    printf '%s\n' "loader_connections=${activity:-absent}"
  fi
  printf '%s\n' "source_admin_sessions_sequential=2"
  printf '%s\n' "target_admin_sessions_sequential=2"
  printf '%s\n' "mp01b_copy=false"
} > "$OUT/probe.txt"

if grep -F -q -f "$SEC/src/admin.pass" "$OUT/probe.txt" || grep -F -q -f "$SEC/src/reader.pass" "$OUT/probe.txt" || grep -F -q -f "$SEC/dst/admin.pass" "$OUT/probe.txt" || grep -F -q -f "$SEC/dst/reader.pass" "$OUT/probe.txt"; then
  echo "probe contains a secret" >&2
  printf '%s\n' 1 > "$OUT/load.exit"
  exit 1
fi
source_id=$(awk -F= '$1=="source_system_identifier"{print $2}' "$OUT/probe.txt" | tr -d '[:space:]')
target_id=$(awk -F= '$1=="target_system_identifier"{print $2}' "$OUT/probe.txt" | tr -d '[:space:]')
if [ "$source_exit" -ne 0 ] || [ "$target_exit" -ne 0 ] || [ "$activity" != "1" ] || [ -z "$source_id" ] || [ "$source_id" = "$target_id" ]; then
  printf '%s\n' 1 > "$OUT/load.exit"
  exit 1
fi
checks=(
  'source_party=2000'
  'source_account=8000'
  'source_entry=90000'
  'source_null_display=118'
  'source_amount_3=1234567890.1234'
  'source_qty_0=-9223372036854775808'
  'source_qty_1=9223372036854775807'
  'source_qty_2=0'
  'source_party_0_display=\N'
  'source_party_0_active=t'
  'source_account_0_party=p00000'
  'target_shop=0'
  'target_entry_regclass=absent'
)
for needed in "${checks[@]}"; do
  if ! grep -F -x "$needed" "$OUT/probe.txt"; then
    echo "probe missing ${needed}" >&2
    printf '%s\n' 1 > "$OUT/load.exit"
    exit 1
  fi
done
printf '%s\n' 0 > "$OUT/load.exit"
echo "fixture_loaded source_rows_measured target_empty"
