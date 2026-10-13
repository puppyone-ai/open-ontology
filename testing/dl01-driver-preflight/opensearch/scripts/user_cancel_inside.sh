#!/bin/bash
# Explicit user cancel, late cancel, and the separate 2s deadline. No customer ledger.
set -u
BIN=/bins
PRIV=/private
FIX=/src/fixtures
OUT=/evidence/proof
mkdir -p "$OUT"
WORK=$(sed -n 's/^  "approved_index": "\([^"]*\)".*/\1/p' "$FIX/cancel-workload.json" | head -n 1)
if [ -z "$WORK" ]; then
  printf 'workload\n' >"$OUT/stopped"
  exit 2
fi
printf '%s\n' "$WORK" >"$OUT/workload-index"

run() {
  name="$1"
  shift
  mkdir -p "$OUT/$name"
  set +e
  "$@" >"$OUT/$name/stdout.txt" 2>"$OUT/$name/stderr.txt"
  code=$?
  set -e
  printf '%s\n' "$code" >"$OUT/$name/exit"
  echo "$name $code"
}

set -e
run cancel "$BIN/os-reader" cancel --private "$PRIV" --index "$WORK" --out "$OUT/cancel" --workload "$FIX/cancel-workload.json"
run cancel-late "$BIN/os-reader" cancel-late --private "$PRIV" --index "$WORK" --out "$OUT/cancel-late" --workload "$FIX/cancel-workload.json"
run deadline "$BIN/os-reader" deadline --private "$PRIV" --index "$WORK" --out "$OUT/deadline" --timeout-ms 20000 --workload "$FIX/cancel-workload.json"
set +e
grep -R --exclude='secret-scan-hits.txt' --exclude='secret-scan.txt' -n -E -e 'password=' -e 'BEGIN PRIVATE' -e 'BEGIN RSA' -e 'BEGIN OPENSSH' -e 'AKIA' "$OUT" >"$OUT/secret-scan-hits.txt"
scan=$?
set -e
if [ "$scan" -eq 1 ]; then
  printf 'secret_hits=0\n' >"$OUT/secret-scan.txt"
elif [ "$scan" -eq 0 ]; then
  printf 'secret_hits=1\n' >"$OUT/secret-scan.txt"
else
  printf 'secret_hits=scan-error\n' >"$OUT/secret-scan.txt"
fi
fail=0
for name in cancel cancel-late deadline; do
  got=$(cat "$OUT/$name/exit" 2>/dev/null || printf 'missing')
  if [ "$got" != "0" ]; then
    echo "phase $name $got"
    fail=1
  fi
done
if ! grep -q 'secret_hits=0' "$OUT/secret-scan.txt"; then
  echo "secret scan failed"
  fail=1
fi
printf 'done\n' >"$OUT/finished"
if [ "$fail" != "0" ]; then
  printf 'phases\n' >"$OUT/stopped"
  exit 2
fi
exit 0
