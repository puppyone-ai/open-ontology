#!/bin/bash
# Focused locked test and clippy for selfbuild-suggestions. No cargo update.
set -u
cd /work/tools/selfbuild-suggestions
rustc --version > /out/rustc.txt 2> /out/rustc.stderr
printf '%s\n' "$?" > /out/rustc.exit
rustup component add rustfmt clippy --toolchain 1.94.0 > /out/components.txt 2> /out/components.stderr
printf '%s\n' "$?" > /out/components.exit
if [ "$(cat /out/components.exit)" != "0" ]; then
  echo "FAILED components=$(cat /out/components.exit)"
  exit 1
fi
rustfmt --edition 2024 --check \
  src/lib.rs src/main.rs src/json.rs src/gateway.rs src/produce.rs tests/producer.rs \
  > /out/rustfmt-check.txt 2> /out/rustfmt-check.stderr
printf '%s\n' "$?" > /out/rustfmt-check.exit
if [ "$(cat /out/rustfmt-check.exit)" != "0" ]; then
  rustfmt --edition 2024 \
    src/lib.rs src/main.rs src/json.rs src/gateway.rs src/produce.rs tests/producer.rs \
    > /out/rustfmt.txt 2> /out/rustfmt.stderr
  printf '%s\n' "$?" > /out/rustfmt.exit
fi
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
test_rc=$(cat /out/test.exit)
clippy_rc=$(cat /out/clippy.exit)
if [ "$test_rc" != "0" ] || [ "$clippy_rc" != "0" ]; then
  echo "FAILED test=$test_rc clippy=$clippy_rc"
  exit 1
fi
echo "DONE test=0 clippy=0"
exit 0
