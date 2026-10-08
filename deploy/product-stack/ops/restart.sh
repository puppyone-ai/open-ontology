#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
mkdir -p "${EVIDENCE}/run"
load_profiles
compose "${COMPOSE_ARGS[@]}" start > "${EVIDENCE}/run/restart.log" 2>&1
printf '%s\n' 0 > "${EVIDENCE}/run/restart.exit"
echo "restart_ok"
