#!/bin/bash
# Read-only check of the seeded source against the independent oracle.
# One admin session for the oracle, then the reader role must reject a write.
# The oracle file is not a model prompt. Passwords are read inside the container.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a419
REPO=/Users/redux/puppy-one/open-ontology
EV=${REPO}/evidence/${RUN_ID}/SP11/logistics-product-loop/fixture/verify
OUT=/tmp/oo-a419-${RUN_ID}/fixture
mkdir -p "$EV"
if [ ! -f "$OUT/oracle.json" ]; then
  echo "oracle file is absent" >&2
  printf '%s\n' 1 > "$EV/verify.exit"
  exit 1
fi
jq -r '.questions[] | "SELECT \u0027" + .id + "\u0027, " + (.sql | sub("^SELECT "; "")) + ";"' "$OUT/oracle.json" > "$EV/oracle-run.sql"
set +e
{
  printf '%s\n' '\set ON_ERROR_STOP on'
  printf '%s\n' '\pset tuples_only on'
  printf '%s\n' '\pset format unaligned'
  printf '%s\n' '\pset fieldsep |'
  printf '%s\n' "SET default_transaction_read_only = on;"
  printf '%s\n' "SET statement_timeout = '30s';"
  printf '%s\n' "SELECT 'identity', current_database(), system_identifier::text FROM pg_control_system();"
  printf '%s\n' "SELECT 'counts', (SELECT count(*) FROM public.hubs)::text, (SELECT count(*) FROM public.shipments)::text, (SELECT count(*) FROM public.tracking_events)::text;"
  printf '%s\n' "SELECT 'keys', (SELECT count(DISTINCT hub_id) FROM public.hubs)::text, (SELECT count(DISTINCT shipment_id) FROM public.shipments)::text, (SELECT count(DISTINCT event_id) FROM public.tracking_events)::text;"
  cat "$EV/oracle-run.sql"
  printf '%s\n' "SELECT 'edges', (SELECT capacity_grams::text FROM public.hubs WHERE hub_id = 1), (SELECT capacity_grams::text FROM public.hubs WHERE hub_id = 2), (SELECT cost_amount::text FROM public.shipments WHERE shipment_id = 1), (SELECT COALESCE(encode(badge, 'hex'), 'null') FROM public.hubs WHERE hub_id = 1), (SELECT COALESCE(encode(badge, 'hex'), 'null') FROM public.hubs WHERE hub_id = 2), (SELECT COALESCE(encode(badge, 'hex'), 'null') FROM public.hubs WHERE hub_id = 3), (SELECT COALESCE(encode(badge, 'hex'), 'null') FROM public.hubs WHERE hub_id = 4);"
  printf '%s\n' "SELECT 'opened', hub_id::text, COALESCE(opened_at::text, 'null') FROM public.hubs WHERE hub_id IN (1, 2, 3, 4, 5, 6) ORDER BY hub_id;"
} | docker exec -i -u postgres oo-a419-src-pg bash -c 'set -eu; export PGPASSWORD="$(cat /secrets/admin.pass)"; exec psql -v ON_ERROR_STOP=1 -U oo_a419_src_admin -d oo_a419_src' > "$EV/source.stdout" 2> "$EV/source.stderr"
src_exit=$?
set -e
printf '%s\n' "$src_exit" > "$EV/source.exit"
set +e
{
  printf '%s\n' '\set ON_ERROR_STOP on'
  printf '%s\n' '\pset tuples_only on'
  printf '%s\n' '\pset format unaligned'
  printf '%s\n' '\pset fieldsep |'
  printf '%s\n' "SELECT 'identity', current_database(), system_identifier::text FROM pg_control_system();"
  printf '%s\n' "SELECT 'hubs', COALESCE(to_regclass('public.hubs')::text, 'absent');"
  printf '%s\n' "SELECT 'shipments', COALESCE(to_regclass('public.shipments')::text, 'absent');"
  printf '%s\n' "SELECT 'events', COALESCE(to_regclass('public.tracking_events')::text, 'absent');"
} | docker exec -i -u postgres oo-a419-dst-pg bash -c 'set -eu; export PGPASSWORD="$(cat /secrets/admin.pass)"; exec psql -v ON_ERROR_STOP=1 -U oo_a419_dst_admin -d oo_a419_dst' > "$EV/target.stdout" 2> "$EV/target.stderr"
dst_exit=$?
set -e
printf '%s\n' "$dst_exit" > "$EV/target.exit"
jq -r '.questions[] | .id + "|" + (.cells[0] | join("|"))' "$OUT/oracle.json" > "$EV/expected.txt"
grep -E '^(event-count|delivered-origin-cost|delayed-air|null-cost|shipment-cost-exact|hub-capacity-bounds)\|' "$EV/source.stdout" > "$EV/actual.txt" || true
if diff -u "$EV/expected.txt" "$EV/actual.txt" > "$EV/compare.diff"; then
  printf '%s\n' match > "$EV/compare.txt"
  compare_exit=0
else
  printf '%s\n' mismatch > "$EV/compare.txt"
  compare_exit=1
fi
printf '%s\n' "$compare_exit" > "$EV/compare.exit"
src_ident=$(awk -F'|' '$1=="identity" {print $2 "|" $3}' "$EV/source.stdout")
dst_ident=$(awk -F'|' '$1=="identity" {print $2 "|" $3}' "$EV/target.stdout")
printf '%s\n' "$src_ident" > "$EV/source-identity.txt"
printf '%s\n' "$dst_ident" > "$EV/target-identity.txt"
ident_exit=0
if [ -z "$src_ident" ] || [ -z "$dst_ident" ] || [ "$src_ident" = "$dst_ident" ]; then
  ident_exit=1
fi
src_db=${src_ident%%|*}
dst_db=${dst_ident%%|*}
src_sys=${src_ident#*|}
dst_sys=${dst_ident#*|}
if [ "$src_db" = "$dst_db" ] || [ "$src_sys" = "$dst_sys" ]; then
  ident_exit=1
fi
printf '%s\n' "$ident_exit" > "$EV/identity.exit"
edge_line=$(grep -E '^edges\|' "$EV/source.stdout" || true)
printf '%s\n' "$edge_line" > "$EV/edges.txt"
edge_exit=0
case "$edge_line" in
  "edges|-9223372036854775808|9223372036854775807|9999999999999999999999999999999999.9999||00|ff|0001ff") edge_exit=0 ;;
  *) edge_exit=1 ;;
esac
printf '%s\n' "$edge_exit" > "$EV/edges.exit"
set +e
docker exec -u postgres -i oo-a419-src-pg bash -s > "$EV/reader.stdout" 2> "$EV/reader.stderr" <<'EOS'
set -eu
psql_ro() {
  psql -v ON_ERROR_STOP=1 --tuples-only --no-align --field-separator='|' "$@"
}
export PGPASSWORD="$(cat /secrets/reader.pass)"
psql_ro -U oo_a419_src_read -d oo_a419_src -c "SELECT 'settings', current_setting('default_transaction_read_only'), current_setting('statement_timeout'), current_setting('lock_timeout');"
psql_ro -U oo_a419_src_read -d oo_a419_src -c "SELECT 'before', count(*)::text FROM public.hubs;"
set +e
psql_ro -U oo_a419_src_read -d oo_a419_src -c "INSERT INTO public.hubs (hub_id, hub_code, hub_name, capacity_grams) VALUES (900001, 'HUB-PROBE', 'probe', 1);" > /tmp/oo-a419-write.stdout 2> /tmp/oo-a419-write.stderr
write_exit=$?
set -e
printf 'write_exit=%s\n' "$write_exit"
cat /tmp/oo-a419-write.stderr
rm -f /tmp/oo-a419-write.stdout /tmp/oo-a419-write.stderr
export PGPASSWORD="$(cat /secrets/admin.pass)"
psql_ro -U oo_a419_src_admin -d oo_a419_src -c "SELECT 'after', count(*)::text FROM public.hubs;"
EOS
reader_exit=$?
set -e
printf '%s\n' "$reader_exit" > "$EV/reader.exit"
write_rejected=1
if ! grep -q '^write_exit=' "$EV/reader.stdout" || grep -q '^write_exit=0$' "$EV/reader.stdout"; then
  write_rejected=0
fi
if ! grep -q '^before|2000$' "$EV/reader.stdout" || ! grep -q '^after|2000$' "$EV/reader.stdout"; then
  write_rejected=0
fi
if ! grep -q '^settings|on|30s|1s$' "$EV/reader.stdout"; then
  write_rejected=0
fi
printf '%s\n' "$write_rejected" > "$EV/write-rejected.txt"
if [ "$src_exit" -ne 0 ] || [ "$dst_exit" -ne 0 ] || [ "$compare_exit" -ne 0 ] || [ "$ident_exit" -ne 0 ] || [ "$edge_exit" -ne 0 ] || [ "$reader_exit" -ne 0 ] || [ "$write_rejected" -ne 1 ]; then
  printf '%s\n' 1 > "$EV/verify.exit"
  exit 1
fi
printf '%s\n' 0 > "$EV/verify.exit"
echo "verify=ok"
