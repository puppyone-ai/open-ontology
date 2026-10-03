#!/bin/bash
# Compile the evidence-only prerequisite against the frozen archive. The archive mount stays read-only.
set -eu
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export RUSTUP_TOOLCHAIN=1.94.0
rm -rf /tmp/src
mkdir -p /target/phases /tmp/src
cp -a /src/. /tmp/src/
test_file=${OO_A125_TEST_FILE:-a125_prereq.rs}
test_bin=${OO_A125_TEST_BIN:-a125_prereq}
cp "/prereq/${test_file}" "/tmp/src/crates/oo-connectors/tests/${test_file}"
cmp /src/crates/oo-connectors/src/snapshot.rs /tmp/src/crates/oo-connectors/src/snapshot.rs
cmp /src/crates/oo-connectors/src/session.rs /tmp/src/crates/oo-connectors/src/session.rs
cmp /src/crates/oo-connectors/src/sql.rs /tmp/src/crates/oo-connectors/src/sql.rs
cd /tmp/src
rustc -Vv
cargo -V
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test "$test_bin" -- --test-threads=1 --nocapture
ec=$?
set -e
printf '%s\n' "$ec" > /target/phases/prereq.exit
exit "$ec"
