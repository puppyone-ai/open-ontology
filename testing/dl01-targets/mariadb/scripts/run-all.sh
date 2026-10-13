#!/bin/bash
# Run the preparation journey and keep every phase exit.
set -euo pipefail
source "$(dirname -- "$0")/lib.sh"
mkdir -p "$OO_EVIDENCE/raw"
worst=0
ran_prepare=0

run_phase() {
  local name=$1
  shift
  set +e
  "$@" >"$OO_EVIDENCE/raw/${name}.stdout" 2>"$OO_EVIDENCE/raw/${name}.stderr"
  local ec=$?
  set -e
  printf '%s\n' "$ec" > "$OO_EVIDENCE/raw/${name}.exit"
  echo "phase=$name exit=$ec"
  if [ "$ec" -ne 0 ] && [ "$worst" -eq 0 ]; then
    worst=$ec
  fi
  return 0
}

run_phase unit-classify python3 "$OO_ROOT/scripts/classify.py" --self-test
run_phase unit-expect python3 "$OO_ROOT/scripts/expect_unit.py"
if [ "$worst" -ne 0 ]; then
  printf '%s\n' "$worst" > "$OO_EVIDENCE/raw/run-all.exit"
  echo "run_all_exit=$worst"
  exit "$worst"
fi
run_phase prepare bash "$OO_ROOT/scripts/prepare.sh"
ran_prepare=$(cat "$OO_EVIDENCE/raw/prepare.exit")
if [ "$ran_prepare" -eq 0 ]; then
  run_phase preflight bash "$OO_ROOT/scripts/preflight.sh"
  run_phase type bash "$OO_ROOT/scripts/type.sh"
  run_phase coverage bash "$OO_ROOT/scripts/coverage.sh"
  run_phase cancel bash "$OO_ROOT/scripts/cancel.sh"
  run_phase audit bash "$OO_ROOT/scripts/audit.sh"
else
  echo "later phases not run because prepare exited $ran_prepare"
fi

if [ "$worst" -eq 0 ]; then
  run_phase cleanup bash "$OO_ROOT/scripts/cleanup.sh"
  if [ "$(cat "$OO_EVIDENCE/raw/cleanup.exit")" -ne 0 ] && [ "$worst" -eq 0 ]; then
    worst=$(cat "$OO_EVIDENCE/raw/cleanup.exit")
  fi
else
  echo "resources retained because a phase failed"
  printf '%s\n' "retained" > "$OO_EVIDENCE/raw/cleanup.exit"
fi

printf '%s\n' "$worst" > "$OO_EVIDENCE/raw/run-all.exit"
echo "run_all_exit=$worst"
exit "$worst"
