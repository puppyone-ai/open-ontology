#!/bin/bash
# Corrected registry-triple check. Root path members are not registry triples.
# Does not build the crate and does not use the network.
set -u
export HOME=/tmp
export RUSTUP_TOOLCHAIN=1.94.0
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH

rustc -vV
if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit
rustc --edition 2024 -D warnings /tools/lock_subset.rs -o /usr/local/bin/lock_subset
printf '%s\n' $? > /evidence/lock-compile.exit
if [ "$(cat /evidence/lock-compile.exit)" -ne 0 ]; then
  exit 1
fi
set +e
/usr/local/bin/lock_subset \
  /src/tools/session-transition/Cargo.lock \
  /src/Cargo.lock \
  > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
lock_exit=$?
set -e
printf '%s\n' "$lock_exit" > /evidence/lock-subset.exit
awk '$1=="triple" { print $2, $3, $4, $5 }' /evidence/lock-subset.stdout > /evidence/lock-triples.txt
if cmp -s /evidence/lock-triples.txt /oracle/registry-triples.txt; then
  printf '%s\n' 0 > /evidence/lock-compare.exit
else
  printf '%s\n' 2 > /evidence/lock-compare.exit
  lock_exit=2
fi
if grep -q 'summary registry_triples=11 missing=0 mismatch=0 direct_pins_present=true' /evidence/lock-subset.stdout; then
  printf '%s\n' 0 > /evidence/lock-summary.exit
else
  printf '%s\n' 2 > /evidence/lock-summary.exit
  lock_exit=2
fi
echo "lock_exit=$lock_exit"
exit "$lock_exit"
