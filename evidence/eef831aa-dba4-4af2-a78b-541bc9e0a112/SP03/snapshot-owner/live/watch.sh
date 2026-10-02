#!/bin/bash
# Silent until the R1 builder reaches a terminal state.
NAME=eef831aa-dba4-4af2-a78b-541bc9e0a112-rust-r1
LOG=/Users/redux/puppy-one/open-ontology/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a112/SP03/snapshot-owner/live/console.log
found=0
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
  if docker inspect "$NAME" >/dev/null 2>&1; then
    found=1
    break
  fi
  sleep 5
done
if [ "$found" -ne 1 ]; then
  echo "FAILED: $NAME did not appear"
  exit 1
fi
while [ "$(docker inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null || echo false)" = "true" ]; do
  sleep 30
done
code=$(docker inspect -f '{{.State.ExitCode}}' "$NAME" 2>/dev/null || echo missing)
status=$(docker inspect -f '{{.State.Status}}' "$NAME" 2>/dev/null || echo missing)
oom=$(docker inspect -f '{{.State.OOMKilled}}' "$NAME" 2>/dev/null || echo missing)
if [ "$code" = "0" ]; then
  echo "DONE: $NAME status=$status exit=0 oom=$oom"
else
  echo "FAILED: $NAME status=$status exit=$code oom=$oom"
  tail -n 60 "$LOG" 2>/dev/null || true
fi
