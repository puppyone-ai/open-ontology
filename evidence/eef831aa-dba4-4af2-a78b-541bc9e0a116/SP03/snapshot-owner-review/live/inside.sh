#!/bin/bash
# Independent oracle, then scoped Clippy. Package live tests stay not-run.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
cd /src
mkdir -p /target/phases
echo "=== identity ==="
rustc -Vv
echo "rustc_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
uname -m
echo "=== product sha256 ==="
set +e
sha256sum -c /evidence/product-sha256.txt
hash_exit=$?
set -e
echo "product_sha_exit=$hash_exit"
printf '%s\n' "$hash_exit" > /target/phases/product-sha.exit
if [ "$hash_exit" -ne 0 ]; then
  printf '%s\n' "$hash_exit" > /target/phases/worst.exit
  exit "$hash_exit"
fi
echo "package_live_pg=not-run"
echo "package_lossless_live=not-run"
echo "snapshot_live=not-run"
echo "lib_retest=not-run"
echo "live_fixture_reason=package tests need OO_PG_FIXTURE; this oracle uses OO_A116_FIXTURE only"
printf '%s\n' "not-run" > /target/phases/package-live.status
echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0
clippy_add_exit=$?
set -e
echo "clippy_add_exit=$clippy_add_exit"
printf '%s\n' "$clippy_add_exit" > /target/phases/clippy-add.exit
echo "=== cargo test --no-run ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test a116_oracle --no-run
compile_exit=$?
set -e
echo "compile_exit=$compile_exit"
printf '%s\n' "$compile_exit" > /target/phases/compile.exit
echo "=== cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "clippy_exit=$clippy_exit"
printf '%s\n' "$clippy_exit" > /target/phases/clippy.exit
oracle_exit=86
if [ "$compile_exit" -eq 0 ] && [ "$clippy_exit" -eq 0 ]; then
  echo "=== cargo test --locked --offline -p oo-connectors --test a116_oracle ==="
  set +e
  cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test a116_oracle -- --test-threads=1 --nocapture
  oracle_exit=$?
  set -e
else
  echo "oracle_skipped=compile_or_clippy_failed"
fi
echo "oracle_exit=$oracle_exit"
printf '%s\n' "$oracle_exit" > /target/phases/oracle.exit
if [ -f /pglog/postgresql.log ]; then
  sha256sum /pglog/postgresql.log > /target/phases/postgresql.log.sha256
  cp /pglog/postgresql.log /target/phases/postgresql.log
  echo "log_copied=yes"
else
  echo "log_copied=no"
  printf '%s\n' "missing" > /target/phases/postgresql.log.sha256
fi
worst=0
for phase_exit in "$clippy_add_exit" "$compile_exit" "$clippy_exit" "$oracle_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/worst.exit
exit "$worst"
