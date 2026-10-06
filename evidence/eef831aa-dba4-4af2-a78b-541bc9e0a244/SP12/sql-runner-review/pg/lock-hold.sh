#!/bin/bash
# Hold ACCESS EXCLUSIVE on the mapped ticket table, then sleep.
# The physical name is checked before it is interpolated. This does not print it.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a231/SP12/sql-runner/pg/lock-hold.sh
# Delta: temporary names are a244.
set -euo pipefail
set +H
name=$(/fixture/psql-admin.sh -d oo_a198 -Atc "SELECT physical_name FROM _oo.physical_mappings WHERE level = 't' AND logical_namespace = 'dispatch' AND logical_table = 'ticket' AND logical_column = '' ORDER BY run_generation DESC LIMIT 1")
printf '%s\n' "$name" > /tmp/oo-a244-physical-name
if [[ ! "$name" =~ ^pt_[a-z0-9_]+$ ]] || [ "${#name}" -ne 63 ]; then
  echo "physical name rejected" >&2
  exit 2
fi
cat > /tmp/oo-a244-lock.sql <<EOF
BEGIN;
LOCK TABLE _oo."${name}" IN ACCESS EXCLUSIVE MODE;
\\! touch /tmp/oo-a244-locked
SELECT pg_sleep(120);
COMMIT;
EOF
exec /fixture/psql-admin.sh -d oo_a198 -v ON_ERROR_STOP=1 -f /tmp/oo-a244-lock.sql
