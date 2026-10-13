#!/bin/bash
# Generate the logistics fixture and load it into the source database only.
# The target database stays empty. Passwords are not printed and are not arguments.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a419
REPO=/Users/redux/puppy-one/open-ontology
CRATE=${REPO}/testing/logistics-product-loop
EV=${REPO}/evidence/${RUN_ID}/SP11/logistics-product-loop/fixture/seed
OUT=/tmp/oo-a419-${RUN_ID}/fixture
BIN=${CRATE}/target/debug/logistics-product-loop
IMAGE=docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
mkdir -p "$EV" "$OUT"
if [ ! -x "$BIN" ]; then
  echo "fixture binary is absent; focused build has not produced it" >&2
  printf '%s\n' 1 > "$EV/seed.exit"
  exit 1
fi
if docker container inspect oo-a419-generate >/dev/null 2>&1 || docker container inspect oo-a419-gold >/dev/null 2>&1; then
  echo "fixture tool container already exists" >&2
  printf '%s\n' 2 > "$EV/seed.exit"
  exit 2
fi
rm -rf "$OUT"
mkdir -p "$OUT"
chmod 700 "$OUT"
set +e
docker run --name oo-a419-generate \
  --platform linux/arm64 \
  --pull never \
  --network none \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 2g \
  --memory-swap 2g \
  --cpus 1 \
  --pids-limit 128 \
  --label oo.project=open-ontology \
  --label oo.owner=a419 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=fixture-tool \
  --mount "type=bind,source=${BIN},target=/tool,readonly" \
  --mount "type=bind,source=${OUT},target=/out" \
  "$IMAGE" \
  /tool generate --out /out > "$EV/generate.log" 2>&1
gen_exit=$?
docker rm oo-a419-generate > "$EV/generate-container-removed.txt" 2>/dev/null
set -e
if [ "$gen_exit" -ne 0 ]; then
  printf '%s\n' "$gen_exit" > "$EV/seed.exit"
  exit "$gen_exit"
fi
set +e
docker run --name oo-a419-gold \
  --platform linux/arm64 \
  --pull never \
  --network none \
  --user 0:0 \
  --security-opt no-new-privileges \
  --cap-drop ALL \
  --memory 1g \
  --memory-swap 1g \
  --cpus 1 \
  --pids-limit 64 \
  --label oo.project=open-ontology \
  --label oo.owner=a419 \
  --label oo.session="${RUN_ID}" \
  --label oo.kind=fixture-tool \
  --mount "type=bind,source=${BIN},target=/tool,readonly" \
  --mount "type=bind,source=${OUT},target=/out" \
  "$IMAGE" \
  /tool gold --out /out > "$EV/gold.log" 2>&1
gold_exit=$?
docker rm oo-a419-gold > "$EV/gold-container-removed.txt" 2>/dev/null
set -e
if [ "$gold_exit" -ne 0 ]; then
  printf '%s\n' "$gold_exit" > "$EV/seed.exit"
  exit "$gold_exit"
fi
set +e
{
  printf '%s\n' '\set ON_ERROR_STOP on'
  printf '%s\n' 'BEGIN;'
  printf '%s\n' "SET statement_timeout = '120s';"
  cat "$OUT/schema.sql"
  printf '%s\n' "COPY public.hubs (hub_id, hub_code, hub_name, region_code, latitude, longitude, capacity_grams, opened_at, badge, note) FROM STDIN WITH (FORMAT text);"
  cat "$OUT/hubs.copy"
  printf '%s\n' '\.'
  printf '%s\n' "COPY public.shipments (shipment_id, origin_hub_id, dest_hub_id, status, transport_mode, weight_grams, cost_amount, created_at, promised_at, seal, comment) FROM STDIN WITH (FORMAT text);"
  cat "$OUT/shipments.copy"
  printf '%s\n' '\.'
  printf '%s\n' "COPY public.tracking_events (event_id, shipment_id, hub_id, status, event_at, recorded_weight_grams, payload, comment) FROM STDIN WITH (FORMAT text);"
  cat "$OUT/tracking_events.copy"
  printf '%s\n' '\.'
  printf '%s\n' 'COMMIT;'
} | docker exec -i -u postgres oo-a419-src-pg bash -c 'set -eu; export PGPASSWORD="$(cat /secrets/admin.pass)"; exec psql -v ON_ERROR_STOP=1 -U oo_a419_src_admin -d oo_a419_src' > "$EV/load.stdout" 2> "$EV/load.stderr"
load_exit=$?
set -e
printf '%s\n' "$load_exit" > "$EV/load.exit"
if [ "$load_exit" -ne 0 ]; then
  printf '%s\n' "$load_exit" > "$EV/seed.exit"
  exit "$load_exit"
fi
set +e
docker exec -u postgres -i oo-a419-src-pg bash -s > "$EV/role.stdout" 2> "$EV/role.stderr" <<'EOS'
set -eu
export PGPASSWORD="$(cat /secrets/admin.pass)"
pw=$(cat /secrets/reader.pass)
case "$pw" in
  *[!0-9a-f]*) echo "reader secret is not hex" >&2; exit 1;;
esac
umask 077
sql=/tmp/oo-a419-reader-role.sql
{
  printf '%s\n' "CREATE ROLE oo_a419_src_read LOGIN PASSWORD '${pw}';"
  # No ALTER ROLE SET. User-source GUCs outside the connector allow-list are startup_option and refuse registration. The connector sets statement_timeout, lock_timeout, and default_transaction_read_only after connect. SELECT-only grants keep the login read-only.
  printf '%s\n' "GRANT CONNECT ON DATABASE oo_a419_src TO oo_a419_src_read;"
  printf '%s\n' "GRANT USAGE ON SCHEMA public TO oo_a419_src_read;"
  printf '%s\n' "GRANT SELECT ON public.hubs, public.shipments, public.tracking_events TO oo_a419_src_read;"
} > "$sql"
psql -v ON_ERROR_STOP=1 -U oo_a419_src_admin -d oo_a419_src -f "$sql"
rm -f "$sql"
EOS
role_exit=$?
set -e
printf '%s\n' "$role_exit" > "$EV/role.exit"
if [ "$role_exit" -ne 0 ]; then
  printf '%s\n' "$role_exit" > "$EV/seed.exit"
  exit "$role_exit"
fi
printf '%s\n' 0 > "$EV/seed.exit"
echo "seeded source=oo-a419-src-pg target_untouched=oo-a419-dst-pg"
