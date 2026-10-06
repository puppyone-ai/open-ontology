#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of the reviewed enum-history suite.
# Fixed archive only. No enum-store overlay. No cargo update. No PostgreSQL.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a289/SP08/calendar-store/linux/precode/inside.sh
# sha256 f0f608c38027a67eba354ca9eef5fcb9dcf6eb65fb0a5b330a221bf32d06c377
# Change: enum-history paths and the 23-test count. awk counter, no Python.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
cd /src

record() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

mkdir -p /evidence/phases

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
uname -a
id

echo "=== archive ==="
if [ ! -f tools/enum-history/Cargo.toml ] || [ ! -f tools/enum-history/Cargo.lock ]; then
  echo "fixed archive missing enum-history" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ ! -f tools/enum-history/CONTRACT.md ] || [ ! -f tools/enum-history/src/lib.rs ]; then
  echo "fixed archive missing enum-history contract or source" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ -e tools/enum-store ]; then
  echo "enum-store present before its code" >&2
  printf '%s\n' 2 > /evidence/absence.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/absence.exit
printf '%s\n' 0 > /evidence/archive.exit

if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/toolchain-aside.txt

echo "=== toolchain ==="
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/toolchain.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/version.exit; exit 2 ;;
esac
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

sha256sum tools/enum-history/Cargo.lock | awk '{print $1}' > /evidence/lock-before.sha256
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

echo "=== cargo fetch locked ==="
set +e
cargo fetch --manifest-path tools/enum-history/Cargo.toml --locked \
  > /evidence/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test locked offline ==="
set +e
cargo test --manifest-path tools/enum-history/Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/test.log 2>&1
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
echo "PHASE test=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

awk '
  /test result:/ {
    passed += $4
    failed += $6
  }
  END { printf "passed=%d failed=%d\n", passed, failed }
' /evidence/test.log > /evidence/test-count.txt
grep -q '^passed=23 failed=0$' /evidence/test-count.txt
printf '%s\n' 0 > /evidence/test-count.exit

sha256sum tools/enum-history/Cargo.lock | awk '{print $1}' > /evidence/lock-after.sha256
before=$(cat /evidence/lock-before.sha256)
after=$(cat /evidence/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "child lock changed" > /evidence/lock-hash.txt
  printf '%s\n' 1 > /evidence/phases/lock-hash.exit
  exit 1
fi
echo "unchanged $after" > /evidence/lock-hash.txt
printf '%s\n' 0 > /evidence/phases/lock-hash.exit
printf '%s\n' 0 > /evidence/smoke.exit
echo "precode=ok"
exit 0
