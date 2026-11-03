#!/bin/sh
# Dispatch one v0.2 task card to a local Codex executor and record the run.
# Usage: execution/v0.2/dispatch.sh <card.md> <lane-worktree>
# Runs land in $OO_V02_RUNS (default: <repo parent>/oo-v02/_runs), outside git.
# Edit by writing a new file and renaming it over this one: a running
# shell keeps reading the old inode. The body is a function so it is fully
# parsed before it runs.
main() {
set -eu
card=$1
worktree=$2
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
runs=${OO_V02_RUNS:-$(dirname "$repo")/oo-v02/_runs}
id=$(basename "$card" .md)-$(date +%Y%m%dT%H%M%S)
run=$runs/$id
mkdir -p "$run"
cat "$here/cards/_common.md" "$card" > "$run/card.md"
git -C "$worktree" rev-parse HEAD > "$run/base-sha"
# Keep lane build directories small: no incremental cache, line tables only.
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=line-tables-only
export CARGO_PROFILE_TEST_DEBUG=line-tables-only
export PATH="$HOME/.cargo/bin:$PATH"
codex exec \
  -m 'cx/gpt-6-astra[1m]' \
  -c model_reasoning_effort=medium \
  --dangerously-bypass-approvals-and-sandbox \
  -C "$worktree" \
  --json \
  -o "$run/report.md" \
  - < "$run/card.md" > "$run/events.jsonl" 2> "$run/stderr.txt" && rc=0 || rc=$?
echo "$rc" > "$run/exit"
git -C "$worktree" rev-parse HEAD > "$run/head-sha"
git -C "$worktree" log --format='%h %s' "$(cat "$run/base-sha")..HEAD" > "$run/commits.txt"
echo "$id exit=$rc"
}
main "$@"
