#!/bin/bash
# Read actual source rows and catalog type rows after the proof. No passwords.
set -u
NAME=oo-a340-pg
OUT=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a340/SP06/numeric-query-review/pg
mkdir -p "$OUT/actual"
if ! docker exec -u postgres "$NAME" pg_isready -U oo_fixture_admin -d postgres >/dev/null 2>&1; then
  echo "postgres is not ready" >&2
  printf '%s\n' 1 > "$OUT/actual/capture.exit"
  exit 1
fi
docker exec -u postgres "$NAME" psql -U oo_fixture_admin -d oo_a327_src -At -F $'\t' -c \
  "SELECT entry_id, amount::text, wide::text, hidden::text, loose IS NULL, \"over\" IS NULL FROM shop.entry ORDER BY entry_id" \
  > "$OUT/actual/source-rows.txt"
src_exit=$?
docker exec -u postgres "$NAME" psql -U oo_fixture_admin -d oo_a327_src -At -F $'\t' -c \
  "SELECT n.nspname, c.relname, a.attname, t.typname, t.typtype, a.atttypmod, t.oid FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_type t ON t.oid = a.atttypid WHERE n.nspname = 'shop' AND c.relname = 'entry' AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum" \
  > "$OUT/actual/source-types.txt"
docker exec -u postgres "$NAME" psql -U oo_fixture_admin -d oo_a327_dst -At -F $'\t' -c \
  "SELECT n.nspname, c.relname, a.attname, t.typname, t.typtype, a.atttypmod, t.oid FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid JOIN pg_namespace n ON n.oid = c.relnamespace JOIN pg_type t ON t.oid = a.atttypid WHERE a.attnum > 0 AND NOT a.attisdropped AND t.typname = 'numeric' ORDER BY n.nspname, c.relname, a.attname" \
  > "$OUT/actual/target-numeric.txt"
docker exec -u postgres "$NAME" psql -U oo_fixture_admin -d oo_a327_dst -At -F $'\t' -c \
  "SELECT n.nspname, t.typname, t.typtype, t.oid FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace WHERE n.nspname = 'pg_catalog' AND t.typname IN ('bool','int2','int4','int8','numeric','text','float4','float8') ORDER BY t.typname" \
  > "$OUT/actual/pg-catalog-oids.txt"
printf '%s\n' "$src_exit" > "$OUT/actual/capture.exit"
echo "capture_exit=$src_exit"
exit "$src_exit"
