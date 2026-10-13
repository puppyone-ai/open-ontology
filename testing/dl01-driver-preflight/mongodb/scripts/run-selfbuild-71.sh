#!/bin/bash
# Call the pinned 71 ELF with the real shipments catalog. Exit 2 when the receipt already exists.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
OUT="$ROOT/evidence/23df6145-4592-4dc0-82df-fd226dd020fd/DL01/mongodb-product/raw/selfbuild-71"
if [[ -f "$OUT/http/create.json" ]]; then
  echo "selfbuild-71 create.json already exists" >&2
  exit 2
fi
exec python3 "$ROOT/testing/dl01-driver-preflight/mongodb/scripts/selfbuild_71_call.py"
