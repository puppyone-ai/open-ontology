#!/bin/bash
# One worker, one already-enqueued pg_effect task. Not the 100-worker campaign.
set -eu
set -o pipefail
PHASE=${OO_PHASE_DIR:-/evidence/linux/run}
mkdir -p "$PHASE"
BIN=/target/debug/job-worker
CFG=/secrets/jw.conf
"$BIN" --config "$CFG" serve --workers 1 --barrier-prefix "" --barrier-target 0 > "$PHASE/serve.stdout" 2> "$PHASE/serve.stderr" &
spid=$!
set +e
"$BIN" --config "$CFG" wait --kind state --dedupe review-once --state succeeded --seconds 40 > "$PHASE/wait.stdout" 2> "$PHASE/wait.stderr"
wcode=$?
kill -TERM "$spid" 2>/dev/null || true
wait "$spid"
scode=$?
set -e
printf '%s\n' "$wcode" > "$PHASE/wait.exit"
printf '%s\n' "$scode" > "$PHASE/serve.exit"
chmod -R a+rwX /evidence || true
if [ "$wcode" -ne 0 ]; then
  exit "$wcode"
fi
exit 0
