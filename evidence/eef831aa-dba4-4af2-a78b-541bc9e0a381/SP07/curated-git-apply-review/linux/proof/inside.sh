#!/bin/bash
# Immutable archive proof. The source mount is read-only. No product edit.
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
if [ -e /src/.git ]; then
  echo "archive unexpectedly contains .git" >&2
  printf '%s\n' 7 > /evidence/archive.exit
  exit 7
fi
printf '%s\n' "no-git" > /evidence/archive-no-git.txt
if [ ! -f /src/tools/curated-git-apply/tests/a381_review.rs ]; then
  echo "review oracle is not on the archive" >&2
  printf '%s\n' 7 > /evidence/archive.exit
  exit 7
fi
sha256sum /src/tools/curated-git-apply/tests/a381_review.rs > /evidence/oracle-mounted.sha256
sha256sum \
  /src/tools/curated-git-apply/CONTRACT.md \
  /src/tools/curated-git-apply/PINNED \
  /src/tools/curated-git-apply/Cargo.toml \
  /src/tools/curated-git-apply/Cargo.lock \
  /src/Cargo.toml \
  /src/rust-toolchain.toml \
  > /evidence/overlay.sha256
cmp /evidence/oracle-mounted.sha256 /evidence/oracle-expected.sha256
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
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
comp=$?
printf '%s\n' "$comp" > /evidence/clippy-component.exit
if [ "$comp" -ne 0 ]; then
  exit 8
fi
export CARGO_NET_OFFLINE=true
cargo +1.94.0 test --locked --offline -- --test-threads=1 --skip pinned_trees_match_the_reviewed_commit > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/test.exit
cargo +1.94.0 clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
set -e
sha256sum Cargo.lock > /evidence/lock-after.txt
cmp /evidence/lock-before.txt /evidence/lock-after.txt
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
exit "$clippy_exit"
