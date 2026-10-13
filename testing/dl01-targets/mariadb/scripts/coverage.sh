#!/bin/bash
# Key coverage for unsigned, decimal, null, JSON, temporal, and binary keys.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"
mkdir -p "$OO_EVIDENCE/raw"

python3 "$OO_ROOT/scripts/expect_unit.py"

oo_reader < "$OO_ROOT/sql/reader-coverage.sql" > "$OO_EVIDENCE/raw/coverage.actual.tsv"
python3 - "$OO_ROOT/expected/coverage.tsv" "$OO_EVIDENCE/raw/coverage.actual.tsv" <<'PY'
import pathlib
import sys

expected = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
actual = pathlib.Path(sys.argv[2]).read_text(encoding="utf-8").splitlines()
if expected != actual:
    print(f"coverage_mismatch expected_lines={len(expected)} actual_lines={len(actual)}")
    for index, (left, right) in enumerate(zip(expected, actual), start=1):
        if left != right:
            print(f"first_diff_line={index}")
            print(f"expected={left}")
            print(f"actual={right}")
            break
    else:
        print("length_diff_only=yes")
    raise SystemExit(1)
print(f"coverage_match_lines={len(actual)}")
PY
echo "coverage=ok"
