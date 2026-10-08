#!/bin/bash
# Rerun the nine author tests that stopped on a missing source .git.
# The archive is read-only. Product bytes are checked against the fixed commit.
# The four independent tests are not in this archive and are not run.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export TMPDIR=/target/tmp
export CARGO_BUILD_JOBS=1
export MAKEFLAGS=-j1
export CARGO_TERM_COLOR=never
mkdir -p "$TMPDIR" "$CARGO_HOME" /evidence
chown -R 501:20 /evidence 2>/dev/null || true
trap 'chown -R 501:20 /evidence 2>/dev/null || true' EXIT

{
  echo "=== identity ==="
  rustc +1.94.0 -vV
  cargo +1.94.0 -V
  git --version
  command -v git
  uname -a
  id
} > /evidence/identity.txt 2>&1
rustc +1.94.0 -vV > /evidence/rustc-version.txt
cargo +1.94.0 -V > /evidence/cargo-version.txt
git --version > /evidence/git-version.txt
uname -m > /evidence/uname.txt
command -v git > /evidence/git-path.txt
if ! grep -q '1.94.0' /evidence/rustc-version.txt; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 4 > /evidence/rustc.exit
  exit 4
fi
printf '%s\n' 0 > /evidence/rustc.exit
grep -q 'aarch64' /evidence/uname.txt
grep -q '/usr/bin/git' /evidence/git-path.txt

if [ ! -d /src/.git ]; then
  echo "source git dir missing" >&2
  printf '%s\n' 7 > /evidence/archive.exit
  exit 7
fi
if [ -e /src/tools/curated-git-apply/tests/a381_review.rs ]; then
  echo "independent oracle must not be on this archive" >&2
  printf '%s\n' 7 > /evidence/archive.exit
  exit 7
fi
printf '%s\n' dir > /evidence/archive-git-dir.txt

fail=0
while IFS="$(printf '\t')" read -r meta path; do
  [ -n "$path" ] || continue
  blob=${meta##* }
  if [ -L "/src/$path" ] || [ ! -f "/src/$path" ]; then
    echo "missing regular file $path" >&2
    fail=1
    continue
  fi
  got=$(git hash-object --no-filters -- "/src/$path")
  if [ "$got" != "$blob" ]; then
    echo "blob mismatch $path" >&2
    fail=1
  fi
done < /evidence/product-ls-tree.txt
if [ "$fail" -ne 0 ]; then
  printf '%s\n' 7 > /evidence/archive.exit
  exit 7
fi
find /src -type f -o -type l | awk '
  BEGIN { n = 0 }
  {
    path = $0
    if (path ~ "^/src/\\.git(/|$)") next
    sub("^/src/", "", path)
    print path
    n++
  }
  END { }
' | sort > /tmp/mounted-files.txt
awk -F '\t' 'NF { print $2 }' /evidence/product-ls-tree.txt | sort > /tmp/expected-files.txt
if ! cmp -s /tmp/expected-files.txt /tmp/mounted-files.txt; then
  echo "mounted file list differs from the fixed tree" >&2
  printf '%s\n' 7 > /evidence/archive.exit
  exit 7
fi
printf '%s\n' 0 > /evidence/archive.exit

cd /src/tools/curated-git-apply
sha256sum Cargo.lock > /evidence/lock-before.txt
set +e
cargo +1.94.0 fetch --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
off=$?
printf '%s\n' "$off" > /evidence/fetch-offline.exit
if [ "$off" -ne 0 ]; then
  cargo +1.94.0 fetch --locked > /evidence/fetch-online.stdout 2> /evidence/fetch-online.stderr
  on=$?
  printf '%s\n' "$on" > /evidence/fetch-online.exit
  if [ "$on" -ne 0 ]; then
    exit 5
  fi
else
  printf '%s\n' skipped > /evidence/fetch-online.exit
fi
export CARGO_NET_OFFLINE=true
cargo +1.94.0 test --locked --offline --test apply -- --test-threads=1 --exact \
  accepted_review_writes_one_dataset_and_leaves_roles_and_hooks \
  replay_recovers_the_same_receipt_and_conflict_does_not_advance \
  refusals_leave_the_ref_in_place \
  kept_approval_does_not_commit \
  column_label_stays_inside_classification \
  two_callers_one_cas_winner \
  dirty_worktree_is_left_byte_for_byte \
  symlink_traversal_and_embedded_repo_do_not_run \
  deadline_kills_the_git_child_before_returning \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/test.exit
set -e
sha256sum Cargo.lock > /evidence/lock-after.txt
cmp /evidence/lock-before.txt /evidence/lock-after.txt
exit "$test_exit"
