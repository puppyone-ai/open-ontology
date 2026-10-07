#!/bin/bash
# Author Clippy on the product tree before the oracle file is added.
set -eu
rustc --version
rustc --version | grep -q "rustc 1.94.0 "
rustup component add clippy --toolchain 1.94.0
cd /build/tools/review-ledger
test ! -f tests/oracle_a367.rs
cargo clippy --locked --offline --all-targets -- -D warnings
