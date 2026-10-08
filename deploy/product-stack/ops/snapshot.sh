#!/bin/bash
# Record secret-tree hash and real table counts. Does not print secret bytes.
set -euo pipefail
source "$(dirname "$0")/lib.sh"
name="${1:?snapshot name}"
mkdir -p "${EVIDENCE}/run"
hash=$(secret_tree_sha)
printf '%s\n' "$hash" > "${EVIDENCE}/run/secret-${name}.sha"
row_snapshot > "${EVIDENCE}/run/rows-${name}.txt" || true
python3 - "${EVIDENCE}/run/rows-${name}.txt" <<'PY'
import json, pathlib, sys
rows = {}
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    if "=" not in line:
        continue
    key, value = line.split("=", 1)
    rows[key] = int(value)
if not rows:
    rows["none"] = 0
pathlib.Path(sys.argv[1] + ".json").write_text(json.dumps(rows, sort_keys=True))
PY
echo "snapshot ${name}"
