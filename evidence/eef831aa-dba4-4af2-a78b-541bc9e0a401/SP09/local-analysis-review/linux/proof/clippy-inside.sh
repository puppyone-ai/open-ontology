#!/bin/bash
# Re-run the focused test and Clippy after the needless-borrow fix.
# The query export, native CLI, and font on the volumes stay in place.
set -u
echo "clippy_resume=1"
export RUSTUP_TOOLCHAIN=1.94.0
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a401
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export PATH="/usr/local/cargo/bin:${PATH}"
rustc --version | tee /evidence/linux/proof/clippy-resume-rustc.txt
set +e
OO_A401_DUCKDB=/native/duckdb \
OO_A401_FONT=/native/NotoSansCJKsc-Regular.otf \
OO_A401_COMPLETE=/evidence/linux/proof/out/complete \
OO_A401_LINEAGE=/evidence/linux/proof/lineage-complete.json \
OO_A401_PARTIAL=/evidence/linux/proof/out/partial \
OO_A401_PARTIAL_LINEAGE=/evidence/linux/proof/lineage-partial.json \
OO_A401_FORMULA=/evidence/linux/proof/out/formula \
cargo test --manifest-path /local-analysis/Cargo.toml --locked --offline --test a401_bounds --target-dir /target/la --jobs 1 -- --test-threads=1 \
  > /evidence/linux/proof/focused-test-final.log 2>&1
test_code=$?
set -e
printf '%s\n' "$test_code" > /evidence/linux/proof/focused-test-final.exit
printf '%s\n' "$test_code" > /evidence/linux/proof/focused-test.exit
printf '%s\n' "$test_code" > /evidence/linux/proof/phases/focused-test.exit
if [ "$test_code" -ne 0 ]; then
  printf '%s\n' "$test_code" > /evidence/linux/proof/chain.exit
  exit "$test_code"
fi
rustup component add clippy > /evidence/linux/proof/clippy-add.log 2>&1 || {
  printf '%s\n' 1 > /evidence/linux/proof/chain.exit
  exit 1
}
set +e
cargo clippy --manifest-path /local-analysis/Cargo.toml --locked --offline --all-targets --target-dir /target/la --jobs 1 -- -D warnings \
  > /evidence/linux/proof/clippy.log 2>&1
clippy_code=$?
set -e
printf '%s\n' "$clippy_code" > /evidence/linux/proof/clippy.exit
printf '%s\n' "$clippy_code" > /evidence/linux/proof/phases/clippy.exit
if [ "$clippy_code" -ne 0 ]; then
  printf '%s\n' "$clippy_code" > /evidence/linux/proof/chain.exit
  exit "$clippy_code"
fi
printf '%s\n' 0 > /evidence/linux/proof/chain.exit
echo "clippy_resume_done"
exit 0
