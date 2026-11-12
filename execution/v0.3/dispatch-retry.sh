#!/bin/sh
# Dispatch a card; if the run ends on a model-relay 502, wait for the relay and resume (max 5).
# Usage: execution/v0.3/dispatch-retry.sh <card.md> <worktree>
set -u
card=$1; worktree=$2; here=$(cd "$(dirname "$0")" && pwd)
runs=$(dirname "$(cd "$here/../.." && pwd)")/oo-v02/_runs
cur=$card
for n in 1 2 3 4 5 6; do
  "$here/dispatch.sh" "$cur" "$worktree"
  run=$(ls -td "$runs"/"$(basename "$cur" .md)"-* | head -1)
  [ "$(cat "$run/exit")" = 0 ] && exit 0
  tail -3 "$run/events.jsonl" | grep -q '502 Bad Gateway\|relay connection failed\|Reconnecting' || exit 1
  [ $n = 6 ] && exit 1
  until curl -s -o /dev/null -m 10 -w '%{http_code}' http://127.0.0.1:18765/v1/models | grep -q 200; do sleep 30; done
  res=$(dirname "$card")/$(basename "$card" .md)-r$n.md
  { echo "## Resume after a model-relay 502 (attempt $n)"; echo; echo "The previous run of this card stopped on a relay 502, not a defect. Its commits are on this branch; uncommitted changes are its unfinished work: review, keep what is correct, finish. Do not redo committed or recorded steps (check git log and the evidence dirs). Commit in small steps."; echo; cat "$card"; } > "$res"
  cur=$res
done
