#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of the fixed cdc-transaction suite.
# Fixed archive only. No cdc-offset-store overlay. No cargo update. No PostgreSQL.
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
if [ ! -f tools/cdc-transaction/Cargo.toml ] || [ ! -f tools/cdc-transaction/Cargo.lock ]; then
  echo "fixed archive missing cdc-transaction" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ -e tools/cdc-offset-store ]; then
  echo "cdc-offset-store present before its code" >&2
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

sha256sum tools/cdc-transaction/Cargo.lock | awk '{print $1}' > /evidence/lock-before.sha256
printf '%s\n' "no" > /evidence/cargo-update-invoked.txt

echo "=== cargo fetch locked ==="
set +e
cargo fetch --manifest-path tools/cdc-transaction/Cargo.toml --locked \
  > /evidence/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test locked offline ==="
set +e
cargo test --manifest-path tools/cdc-transaction/Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

sha256sum tools/cdc-transaction/Cargo.lock | awk '{print $1}' > /evidence/lock-after.sha256
before=$(cat /evidence/lock-before.sha256)
after=$(cat /evidence/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/lock-hash.txt
  printf '%s\n' 1 > /evidence/phases/lock-hash.exit
  printf '%s\n' 1 > /evidence/smoke.exit
  exit 1
fi
echo "unchanged $after" > /evidence/lock-hash.txt
printf '%s\n' 0 > /evidence/phases/lock-hash.exit
count=$(grep -c '^test ' /evidence/test.log || true)
printf '%s\n' "$count" > /evidence/test-count.txt
printf '%s\n' 0 > /evidence/test-count.exit
printf '%s\n' 0 > /evidence/smoke.exit
echo "proof=ok tests=$count"
exit 0
