#!/bin/bash
# Run the OpenSearch target preparation. Does not compile Rust.
set -u
ROOT=/Users/redux/puppy-one/open-ontology/testing/dl01-targets/opensearch
EVIDENCE=/Users/redux/puppy-one/open-ontology/evidence/b53c75ff-62f4-4869-8428-a605d263cb43/DL01/opensearch
mkdir -p "$EVIDENCE/raw/run"
cd "$ROOT"
rc=0
python3 "$ROOT/scripts/render_private.py" > "$EVIDENCE/raw/run/render.stdout" 2> "$EVIDENCE/raw/run/render.stderr" || rc=$?
printf '%s\n' "$rc" > "$EVIDENCE/raw/run/render.exit"
if [ "$rc" -ne 0 ]; then
  exit "$rc"
fi
python3 "$ROOT/scripts/admit_up.py" > "$EVIDENCE/raw/run/admit.stdout" 2> "$EVIDENCE/raw/run/admit.stderr" || rc=$?
printf '%s\n' "$rc" > "$EVIDENCE/raw/run/admit.exit"
if [ "$rc" -ne 0 ]; then
  bash "$ROOT/scripts/down.sh" || true
  exit "$rc"
fi
python3 "$ROOT/scripts/journey.py" > "$EVIDENCE/raw/run/journey.stdout" 2> "$EVIDENCE/raw/run/journey.stderr" || rc=$?
printf '%s\n' "$rc" > "$EVIDENCE/raw/run/journey.exit"
bash "$ROOT/scripts/down.sh" || true
exit "$rc"
