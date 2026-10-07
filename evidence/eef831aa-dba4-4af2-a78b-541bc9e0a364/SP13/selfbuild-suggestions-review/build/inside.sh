#!/bin/bash
# Focused locked tests and clippy, then the independent probes.
# No cargo update and no lock rewrite.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
mkdir -p /target/tmp /cargo-home
export TMPDIR=/target/tmp
cd /work/tools/selfbuild-suggestions
rustc --version > /out/rustc.txt 2> /out/rustc.stderr
printf '%s\n' "$?" > /out/rustc.exit
rustup component add clippy --toolchain 1.94.0 > /out/components.txt 2> /out/components.stderr
printf '%s\n' "$?" > /out/components.exit
if [ "$(cat /out/components.exit)" != "0" ]; then
  echo "FAILED components=$(cat /out/components.exit)"
  exit 1
fi
cp Cargo.lock /out/Cargo.lock.frozen
cargo fetch --locked > /out/fetch.txt 2> /out/fetch.stderr
printf '%s\n' "$?" > /out/fetch.exit
if [ "$(cat /out/fetch.exit)" != "0" ]; then
  echo "FAILED fetch=$(cat /out/fetch.exit)"
  exit 1
fi
cargo test --offline --locked -- --test-threads=1 > /out/test.txt 2> /out/test.stderr
printf '%s\n' "$?" > /out/test.exit
cargo clippy --offline --locked --all-targets -- -D warnings > /out/clippy.txt 2> /out/clippy.stderr
printf '%s\n' "$?" > /out/clippy.exit
if ! cmp -s Cargo.lock /out/Cargo.lock.frozen; then
  echo "FAILED lock rewritten"
  printf '%s\n' 1 > /out/lock.exit
  exit 1
fi
printf '%s\n' 0 > /out/lock.exit
test_rc=$(cat /out/test.exit)
clippy_rc=$(cat /out/clippy.exit)
if [ "$test_rc" != "0" ] || [ "$clippy_rc" != "0" ]; then
  echo "FAILED test=$test_rc clippy=$clippy_rc"
  exit 1
fi
rm -rf /tmp/sbs-review
cp -a /work/tools/selfbuild-suggestions /tmp/sbs-review
cp /probe/independent.rs /tmp/sbs-review/tests/independent.rs
cd /tmp/sbs-review
if ! cmp -s Cargo.lock /out/Cargo.lock.frozen; then
  echo "FAILED copy lock differs"
  exit 1
fi
OO_A364_OUT=/out cargo test --offline --locked --test independent -- --test-threads=1 > /out/independent.txt 2> /out/independent.stderr
printf '%s\n' "$?" > /out/independent.exit
if ! cmp -s Cargo.lock /out/Cargo.lock.frozen; then
  echo "FAILED independent lock rewritten"
  exit 1
fi
if [ "$(cat /out/independent.exit)" != "0" ]; then
  echo "FAILED independent=$(cat /out/independent.exit)"
  exit 1
fi
test -s /out/live-command.json
grep -q 'SELECT party_key FROM shop.account WHERE party_key IS NOT NULL LIMIT 1' /out/live-command.json
echo "DONE test=0 clippy=0 independent=0"
exit 0
