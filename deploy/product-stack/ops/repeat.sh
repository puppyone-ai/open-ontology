#!/bin/bash
# Second up. Install marker blocks schema changes.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run"
load_profiles
if [ ! -f "${STATE}/install/marker" ]; then
  echo "refusing repeat before the first install attempt" >&2
  printf '%s\n' 2 > "${EVIDENCE}/run/repeat.exit"
  exit 2
fi
compose "${COMPOSE_ARGS[@]}" up -d > "${EVIDENCE}/run/repeat.log" 2>&1
printf '%s\n' 0 > "${EVIDENCE}/run/repeat.exit"
echo "repeat_ok"
