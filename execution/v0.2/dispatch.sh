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
free_gb() { df -g / | awk 'NR==2{print $4}'; }
# A full disk took Docker Desktop down once (ledger); refuse to start below 20 GB.
if [ "$(free_gb)" -lt "${OO_MIN_FREE_GB:-20}" ]; then
  echo "refusing to dispatch: $(free_gb) GB free" >&2; exit 3
fi
mkdir -p "$run"
cat "$here/cards/_common.md" "$card" > "$run/card.md"
git -C "$worktree" rev-parse HEAD > "$run/base-sha"
# Keep lane build directories small: no incremental cache, line tables only.
export CARGO_INCREMENTAL=0
# Several lanes compile at once; full parallelism each thrashes the host.
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-6}
export CARGO_PROFILE_DEV_DEBUG=line-tables-only
export CARGO_PROFILE_TEST_DEBUG=line-tables-only
export PATH="$HOME/.cargo/bin:$PATH"
( while sleep 60; do
    if [ "$(free_gb)" -lt "${OO_STOP_FREE_GB:-8}" ]; then
      echo "disk_guard: $(free_gb) GB free" > "$run/disk-guard"
      pkill -f "codex exec .* -C $worktree" ; exit 0
    fi
  done ) &
guard=$!
codex exec \
  -m 'cx/gpt-6-astra[1m]' \
  -c model_reasoning_effort=medium \
  --dangerously-bypass-approvals-and-sandbox \
  -C "$worktree" \
  --json \
  -o "$run/report.md" \
  - < "$run/card.md" > "$run/events.jsonl" 2> "$run/stderr.txt" && rc=0 || rc=$?
kill "$guard" 2>/dev/null || true
echo "$rc" > "$run/exit"
git -C "$worktree" rev-parse HEAD > "$run/head-sha"
git -C "$worktree" log --format='%h %s' "$(cat "$run/base-sha")..HEAD" > "$run/commits.txt"
# Host disk is small: a finished lane keeps no build directory; the next
# dispatch or the integration gate rebuilds it.
# Every Cargo build directory (tools/* and evidence/ ones too), recognised by
# the CACHEDIR.TAG Cargo writes.
find "$worktree" -maxdepth 5 -type d -name target -exec test -e {}/CACHEDIR.TAG \; -prune -exec rm -rf {} +
echo "$id exit=$rc"
}
main "$@"
