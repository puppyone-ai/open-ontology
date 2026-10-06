#!/bin/bash
# Recompile the corrected review oracle and rerun registry + source.
# The preoverlay source oracle looked for the refusal sentence in lib.rs.
# That sentence is in child.rs. This run does not replace preoverlay-msrv.
# Std-only rustc. No cargo fetch. Network is none.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
cd /work
echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
echo "=== review oracle ==="
set +e
rustc --edition 2021 -O -o /usr/local/bin/review-oracle /oracle/review_oracle.rs > /proof/oracle-rustc.stdout 2> /proof/oracle-rustc.stderr
oracle_rustc_exit=$?
set -e
printf '%s\n' "$oracle_rustc_exit" > /proof/oracle-rustc.exit
echo "PHASE oracle_rustc_exit=$oracle_rustc_exit"
if [ "$oracle_rustc_exit" -ne 0 ]; then
  exit "$oracle_rustc_exit"
fi
set +e
/usr/local/bin/review-oracle registry /work/Cargo.lock /work/tests/fixtures/registry-triples.txt /root-lock/Cargo.lock /work/Cargo.toml > /proof/registry-oracle.stdout 2> /proof/registry-oracle.stderr
registry_exit=$?
/usr/local/bin/review-oracle source /work > /proof/source-oracle.stdout 2> /proof/source-oracle.stderr
source_exit=$?
set -e
printf '%s\n' "$registry_exit" > /proof/registry-oracle.exit
printf '%s\n' "$source_exit" > /proof/source-oracle.exit
echo "PHASE registry_oracle_exit=$registry_exit source_oracle_exit=$source_exit"
fail=0
if [ "$registry_exit" -ne 0 ]; then
  fail=$registry_exit
fi
if [ "$fail" -eq 0 ] && [ "$source_exit" -ne 0 ]; then
  fail=$source_exit
fi
exit "$fail"
