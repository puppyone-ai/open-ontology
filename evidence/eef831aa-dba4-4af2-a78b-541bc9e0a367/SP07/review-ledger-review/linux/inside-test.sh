#!/bin/bash
# Author locked tests on the fresh archive. No lock update.
set -eu
rustc --version
rustc --version | grep -q "rustc 1.94.0 "
cd /build/tools/review-ledger
test -f Cargo.lock
cargo test --locked --lib --test contract --test live -- --test-threads=1
