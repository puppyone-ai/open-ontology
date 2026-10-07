#!/bin/bash
# Author restart phase. Reads the history marker. Does not rewrite the queue.
set -eu
rustc --version
rustc --version | grep -q "rustc 1.94.0 "
cd /build/tools/review-ledger
test ! -f tests/oracle_a367.rs
cargo test --locked --offline --test live -- --test-threads=1
