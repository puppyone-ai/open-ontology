#!/bin/bash
# Dispatcher. Shell only orchestrates the linux oracle and the compose project.
set -euo pipefail
cmd="${1:-}"
shift || true
case "$cmd" in
  init|preflight|standard-gate|images|up|health|repeat|stop|restart|cleanup|journey|sbom|phase|snapshot)
    exec "$(dirname "$0")/${cmd}.sh" "$@"
    ;;
  *)
    echo "usage: oo.sh init|preflight|standard-gate|images|up|health|repeat|stop|restart|cleanup|journey|sbom|phase|snapshot" >&2
    exit 1
    ;;
esac
