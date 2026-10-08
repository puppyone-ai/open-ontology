#!/bin/bash
# standard and minimal require runsc. A missing runsc refuses. runc is not a substitute.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run"
set +e
"$(dirname "$0")/preflight.sh" standard
standard=$?
"$(dirname "$0")/preflight.sh" minimal
minimal=$?
"$(dirname "$0")/preflight.sh" observability
observability=$?
set -e
printf '%s\n' "$standard" > "${EVIDENCE}/run/standard-gate.exit"
printf '%s\n' "$minimal" > "${EVIDENCE}/run/minimal-gate.exit"
printf '%s\n' "$observability" > "${EVIDENCE}/run/observability-gate.exit"
if [ "$standard" -eq 0 ] || [ "$minimal" -eq 0 ]; then
  echo "runsc gate did not fail closed" >&2
  exit 1
fi
echo "standard_and_minimal_refused"
exit 2
