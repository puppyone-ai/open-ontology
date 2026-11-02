#!/usr/bin/env bash
# Removes the two owned containers. Secrets and certificates are kept (see README).
set -uo pipefail
docker rm -f oo-cb-T9-mysql57 oo-cb-T9-pg17 >/dev/null 2>&1 && echo "removed oo-cb-T9-mysql57 oo-cb-T9-pg17" || echo "nothing to remove"
