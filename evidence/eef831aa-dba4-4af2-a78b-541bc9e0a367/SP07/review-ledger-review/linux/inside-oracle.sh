#!/bin/bash
# Independent oracle, then Clippy of that test only.
set -eu
rustc --version
rustc --version | grep -q "rustc 1.94.0 "
cd /build/tools/review-ledger
test -f tests/oracle_a367.rs
cargo test --locked --offline --test oracle_a367 -- --test-threads=1 --nocapture
rustup component add clippy --toolchain 1.94.0
cargo clippy --locked --offline --test oracle_a367 -- -D warnings
