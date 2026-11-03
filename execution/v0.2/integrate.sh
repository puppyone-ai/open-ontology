#!/bin/sh
# Gate and fast-forward one lane branch into the v0.2 integration branch.
# Usage: execution/v0.2/integrate.sh <lane-branch> [extra test command]
# Run from the integration worktree. Refuses on any failed gate; never
# merges with a merge commit and never force-pushes.
set -eu
lane=$1
extra=${2:-}
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
cd "$repo"
target=$(git rev-parse --abbrev-ref HEAD)
[ "$target" = "v0.2/product-loops" ] || { echo "run on v0.2/product-loops, not $target" >&2; exit 2; }
[ -z "$(git status --porcelain)" ] || { echo "integration worktree is dirty" >&2; exit 2; }
lane_wt=$(git worktree list --porcelain | awk -v b="refs/heads/$lane" '/^worktree /{w=$2} $0=="branch "b{print w}')
[ -n "$lane_wt" ] || { echo "no worktree for $lane" >&2; exit 2; }
[ -z "$(git -C "$lane_wt" status --porcelain)" ] || { echo "lane worktree $lane_wt has uncommitted changes" >&2; exit 2; }

echo "== rebase $lane onto $target"
git -C "$lane_wt" rebase "$target"
range="$target..$lane"
git log --format='%h %s' "$range"
[ -n "$(git rev-list "$range")" ] || { echo "nothing to integrate" >&2; exit 2; }

echo "== commit message gate"
for c in $(git rev-list "$range"); do
  subject=$(git log -1 --format=%s "$c")
  body=$(git log -1 --format=%B "$c")
  echo "$subject" | grep -Eq '^(feat|fix|refactor|test|docs|build|ci|chore|perf)(\([a-z0-9/._-]+\))?: .{1,72}$' \
    || { echo "bad subject in $c: $subject" >&2; exit 1; }
  echo "$body" | grep -q '^Plan: ' || { echo "missing Plan trailer in $c" >&2; exit 1; }
done

echo "== secret and private-term gate"
terms=${OO_PRIVATE_TERMS_FILE:-$HOME/.config/open-ontology/private-terms.txt}
added=$(git diff "$target...$lane" -U0 | grep -E '^\+' || true)
if echo "$added" | grep -Eq '(^|[^A-Za-z])sk-[A-Za-z0-9]{12,}|(ooct|oost)_[A-Za-z0-9_-]{20,}|BEGIN [A-Z ]*PRIVATE KEY|/Users/[A-Za-z]'; then
  echo "secret-like or absolute-path content added" >&2; exit 1
fi
if [ -f "$terms" ] && echo "$added" | grep -qiF -f "$terms"; then
  echo "private term added" >&2; exit 1
fi
big=$(git diff --numstat "$target...$lane" | awk '$1=="-"{print $3}')
for f in $big; do
  size=$(git cat-file -s "$lane:$f" 2>/dev/null || echo 0)
  [ "$size" -le 1048576 ] || { echo "binary over 1 MiB: $f" >&2; exit 1; }
done

echo "== build and test gate (on the rebased lane tree)"
( cd "$lane_wt"
  cargo check --workspace --locked
  touched=$(git diff --name-only "$target...$lane" | sed -n 's#^crates/\([^/]*\)/.*#\1#p' | sort -u)
  for c in $touched; do
    case "$c" in
      oo-connectors) cargo test --locked -p oo-connectors --all-features --lib ;;
      xtask|test-suites) ;;
      *) cargo test --locked -p "$c" ;;
    esac
  done
  [ -z "$extra" ] || sh -c "$extra"
  python3 specs/scripts/check_spec.py
  python3 specs/scripts/check_plan.py
  python3 specs/scripts/check_v02_plan.py
)

echo "== fast-forward"
git merge --ff-only "$lane"
git log --oneline -1
