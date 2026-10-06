#!/bin/bash
# Author suite on the fixed archive, before the review overlay is added.
# Linux arm64 Rust 1.94. No PostgreSQL. cp -R, not cp -a.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export TMPDIR=/work/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target/tool
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CDC_SPOOL_HWM_OUT=/evidence/author-seal-hwm.txt

mkdir -p /work/tmp /evidence

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
id

echo "=== rustc version gate ==="
if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  echo "toolchain is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

echo "=== overlay absent ==="
if [ -e /tool/tests/a299_boundary.rs ]; then
  echo "fixed archive already contains the review overlay" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit

echo "=== product copy ==="
rm -rf /work/cdc-spool
mkdir -p /work/cdc-spool
cp -R /tool/. /work/cdc-spool/
cd /work/cdc-spool
if [ ! -f Cargo.lock ] || [ ! -f src/lib.rs ] || [ ! -f src/main.rs ] || [ ! -f CONTRACT.md ]; then
  echo "product crate is incomplete" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if [ -e tests/a299_boundary.rs ]; then
  echo "work copy contains the review overlay" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if ! grep -q 'name = "cdc-spool"' Cargo.lock; then
  echo "lock does not name cdc-spool" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
if grep -q 'name = "cdc-transaction"' Cargo.lock; then
  echo "lock still names cdc-transaction" >&2
  printf '%s\n' 2 > /evidence/source.exit
  exit 2
fi
find Cargo.toml Cargo.lock CONTRACT.md src tests -type f | sort | xargs sha256sum > /evidence/source-sha256.txt
mkdir -p /evidence/source
cp -R Cargo.toml Cargo.lock CONTRACT.md src tests /evidence/source/
cp Cargo.lock /evidence/lock-before-sha256
sha256sum Cargo.lock > /evidence/lock-before.txt
printf '%s\n' 0 > /evidence/source.exit

echo "=== fetch ==="
set +e
cargo fetch --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-add.stdout 2> /evidence/clippy-add.stderr
clippy_add_exit=$?
printf '%s\n' "$clippy_add_exit" > /evidence/clippy-add.exit
echo "PHASE clippy_add_exit=$clippy_add_exit"
if [ "$clippy_add_exit" -ne 0 ]; then
  exit "$clippy_add_exit"
fi

echo "=== clippy ==="
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

echo "=== test ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== author totals ==="
summary=$(awk '
/^test result:/ {
  n = split($0, a, /[; ]+/)
  for (i = 1; i <= n; i++) {
    if (a[i] == "passed") p += a[i - 1]
    if (a[i] == "failed") f += a[i - 1]
  }
}
END { printf "%s %s\n", p + 0, f + 0 }
' /evidence/test.stdout)
printf '%s\n' "$summary" > /evidence/author-totals.txt
echo "PHASE author_totals=$summary"
set -- $summary
if [ "$1" != "34" ] || [ "$2" != "0" ]; then
  echo "author suite was not 34 passed and 0 failed" >&2
  exit 4
fi
if [ ! -s /evidence/author-seal-hwm.txt ]; then
  echo "author seal VmHWM was not recorded" >&2
  exit 4
fi

echo "=== registry triples ==="
set +e
rustc --edition 2024 -D warnings /review-tools/lock_subset.rs -o /work/lock_subset > /evidence/registry-rustc.stdout 2> /evidence/registry-rustc.stderr
reg_rustc=$?
printf '%s\n' "$reg_rustc" > /evidence/registry-rustc.exit
echo "PHASE registry_rustc_exit=$reg_rustc"
if [ "$reg_rustc" -ne 0 ]; then
  exit "$reg_rustc"
fi
/work/lock_subset /work/cdc-spool/Cargo.lock /root.lock > /evidence/registry.txt 2> /evidence/registry.stderr
reg_exit=$?
printf '%s\n' "$reg_exit" > /evidence/registry.exit
echo "PHASE registry_exit=$reg_exit"
if [ "$reg_exit" -ne 0 ]; then
  exit "$reg_exit"
fi
sha256sum Cargo.lock > /evidence/lock-after.txt
if ! cmp -s /evidence/lock-before-sha256 Cargo.lock; then
  echo "Cargo.lock changed during the author suite" >&2
  exit 4
fi
echo PREOVERLAY_OK
exit 0
