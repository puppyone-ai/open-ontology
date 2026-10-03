#!/bin/bash
# Offline clippy and the a121 oracle. The archive was fetched by the smoke builder.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
cd /src
mkdir -p /target/phases
echo "=== identity ==="
uname -m
rustc -Vv
echo "rustc_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
echo "=== product hashes ==="
set +e
sha256sum -c /evidence/product-sha256.txt
hash_exit=$?
set -e
echo "product_hash_exit=$hash_exit"
printf '%s\n' "$hash_exit" > /target/phases/product-hash.exit
echo "=== clippy component ==="
set +e
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0
  add_exit=$?
else
  add_exit=0
fi
set -e
echo "clippy_add_exit=$add_exit"
printf '%s\n' "$add_exit" > /target/phases/clippy-add.exit
echo "=== compile oracle ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test a121_oracle --no-run
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
echo "=== oracle ==="
set +e
if [ "$hash_exit" -ne 0 ] || [ "$add_exit" -ne 0 ] || [ "$compile_exit" -ne 0 ] || [ "$clippy_exit" -ne 0 ]; then
  echo "oracle=skipped_after_build_failure"
  printf '%s\n' skipped > /target/phases/oracle.status
  oracle_exit=0
else
  cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --test a121_oracle -- --test-threads=1 --nocapture
  oracle_exit=$?
  printf '%s\n' ran > /target/phases/oracle.status
fi
set -e
echo "oracle_exit=$oracle_exit"
printf '%s\n' "$oracle_exit" > /target/phases/oracle.exit
echo "lib_retest=not-run"
echo "package_live_pg=not-run"
echo "snapshot_live=not-run"
worst=0
for phase_exit in "$hash_exit" "$add_exit" "$compile_exit" "$clippy_exit" "$oracle_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/worst.exit
exit "$worst"
