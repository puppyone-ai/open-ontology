#!/bin/bash
# Statement zero-write proof for oo_reader. The reader never sends DML or DDL.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"
mkdir -p "$OO_EVIDENCE/raw"

python3 "$OO_ROOT/scripts/classify.py" --self-test

denial_out=$(mktemp)
denial_err=$(mktemp)
set +e
oo_reader <<'SQL' >"$denial_out" 2>"$denial_err"
SELECT COUNT(*) FROM mysql.global_priv;
SQL
denial_ec=$?
set -e
echo "reader_privilege_table_exit=$denial_ec"
# Keep the denial text only after a secret scan. A denied SELECT has no rows.
if ! python3 "$OO_ROOT/scripts/secret_scan.py" "$OO_SECRET_DIR" "$denial_err"; then
  rm -f "$denial_out" "$denial_err"
  echo "reader denial stderr contained a secret and was not copied" >&2
  exit 1
fi
cat "$denial_err" > "$OO_EVIDENCE/raw/reader-denied.stderr"
rm -f "$denial_out" "$denial_err"
if [ "$denial_ec" -eq 0 ]; then
  echo "reader could read mysql.global_priv" >&2
  exit 1
fi
if ! grep -Eiq 'denied|access|1142|1044' "$OO_EVIDENCE/raw/reader-denied.stderr"; then
  echo "reader denial did not report an access error" >&2
  exit 1
fi

oo_dba <<'SQL' > "$OO_EVIDENCE/raw/reader-log.tsv"
SELECT CONCAT(command_type, CHAR(9), REPLACE(REPLACE(REPLACE(IFNULL(argument, ''), CHAR(9), ' '), CHAR(10), ' '), CHAR(13), ' '))
FROM mysql.general_log
WHERE user_host LIKE 'oo_reader%'
ORDER BY event_time, thread_id;
SQL
python3 "$OO_ROOT/scripts/secret_scan.py" "$OO_SECRET_DIR" "$OO_EVIDENCE/raw/reader-log.tsv"
python3 "$OO_ROOT/scripts/classify.py" "$OO_EVIDENCE/raw/reader-log.tsv" | tee "$OO_EVIDENCE/raw/reader-log.summary"
grep -q 'write=0' "$OO_EVIDENCE/raw/reader-log.summary"
grep -q 'unexpected=0' "$OO_EVIDENCE/raw/reader-log.summary"
grep -q 'mysql.global_priv' "$OO_EVIDENCE/raw/reader-log.tsv"
echo "audit=ok"
