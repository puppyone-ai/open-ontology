#!/bin/bash
# Reader type projection against the checked-in oracle, then DBA negative rejects.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"
mkdir -p "$OO_EVIDENCE/raw"

oo_reader < "$OO_ROOT/sql/reader-types.sql" > "$OO_EVIDENCE/raw/types.actual.tsv"
python3 - "$OO_ROOT/expected/types.tsv" "$OO_EVIDENCE/raw/types.actual.tsv" <<'PY'
import pathlib
import sys

expected = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
actual = pathlib.Path(sys.argv[2]).read_text(encoding="utf-8").splitlines()
if expected != actual:
    print(f"type_mismatch expected_lines={len(expected)} actual_lines={len(actual)}")
    for index, (left, right) in enumerate(zip(expected, actual), start=1):
        if left != right:
            print(f"first_diff_line={index}")
            print(f"expected={left}")
            print(f"actual={right}")
            break
    else:
        print("length_diff_only=yes")
    raise SystemExit(1)
print(f"type_match_lines={len(actual)}")
PY

run_reject() {
  local name=$1
  local sql=$2
  local expect=$3
  local err="$OO_EVIDENCE/raw/negative-${name}.stderr"
  set +e
  printf '%s\n' "USE oo_synth; $sql" | oo_dba >/dev/null 2>"$err"
  local ec=$?
  set -e
  echo "negative_${name}_exit=$ec"
  if [ "$ec" -eq 0 ]; then
    echo "negative $name was accepted" >&2
    return 1
  fi
  if ! grep -Eiq "$expect" "$err"; then
    echo "negative $name did not report the expected reject" >&2
    return 1
  fi
}

run_reject unsigned_negative "INSERT INTO k_unsigned (id, label) VALUES (-1, 'neg');" '1264|out of range'
run_reject invalid_json "INSERT INTO k_json (id, doc) VALUES (99, 'not-json');" '4025|CONSTRAINT'
run_reject zero_date "INSERT INTO k_zero_probe (id, d) VALUES (1, '0000-00-00');" '1292|1525|Incorrect date|zero date'

counts=$(oo_dba <<'SQL'
USE oo_synth;
SELECT CONCAT('neg_unsigned|', COUNT(*)) FROM k_unsigned WHERE label = 'neg';
SELECT CONCAT('neg_json|', COUNT(*)) FROM k_json WHERE id = 99;
SELECT CONCAT('neg_zero|', COUNT(*)) FROM k_zero_probe;
SQL
)
printf '%s\n' "$counts" | tee "$OO_EVIDENCE/raw/negative-counts.txt"
printf '%s\n' "$counts" | grep -qx 'neg_unsigned|0'
printf '%s\n' "$counts" | grep -qx 'neg_json|0'
printf '%s\n' "$counts" | grep -qx 'neg_zero|0'
echo "type=ok"
