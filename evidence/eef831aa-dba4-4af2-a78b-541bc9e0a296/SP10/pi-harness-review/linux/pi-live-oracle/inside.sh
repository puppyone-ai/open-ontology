#!/bin/bash
# Compile the corrected oracle and check the finished live proof.
# hostconfig.txt is already in /proof. Network none. No Pi process.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
echo "=== identity ==="
rustc -vV
set +e
rustc --edition 2021 -O -o /usr/local/bin/review-oracle /oracle/review_oracle.rs > /out/oracle-rustc.stdout 2> /out/oracle-rustc.stderr
oracle_rustc_exit=$?
set -e
printf '%s\n' "$oracle_rustc_exit" > /out/oracle-rustc.exit
echo "PHASE oracle_rustc_exit=$oracle_rustc_exit"
if [ "$oracle_rustc_exit" -ne 0 ]; then
  exit "$oracle_rustc_exit"
fi
set +e
/usr/local/bin/review-oracle live /proof > /out/live-oracle.stdout 2> /out/live-oracle.stderr
oracle_exit=$?
set -e
printf '%s\n' "$oracle_exit" > /out/live-oracle.exit
echo "PHASE live_oracle_exit=$oracle_exit"
exit "$oracle_exit"
