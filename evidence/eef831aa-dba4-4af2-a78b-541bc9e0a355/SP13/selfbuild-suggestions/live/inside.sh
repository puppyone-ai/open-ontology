#!/bin/bash
# Ignored live test only. Offline, locked, one thread. No lock rewrite.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export OO_A355_QUERY_PG=/gw-target/debug/query-pg
export OO_A355_REQUEST=/secrets/request.json
mkdir -p /target/tmp /out
export TMPDIR=/target/tmp
cd /work/tools/selfbuild-suggestions
if [ -f /work/rust-toolchain.toml ] || [ -f rust-toolchain.toml ]; then
  echo "FAILED toolchain file is present"
  exit 1
fi
rustc -vV > /out/rustc.txt 2> /out/rustc.stderr
printf '%s\n' "$?" > /out/rustc.exit
if ! grep -q '1.94.0' /out/rustc.txt; then
  echo "FAILED rustc"
  exit 1
fi
if [ ! -f "$OO_A355_QUERY_PG" ] || [ -L "$OO_A355_QUERY_PG" ]; then
  echo "FAILED query-pg is not a regular file"
  exit 1
fi
cp Cargo.lock /out/Cargo.lock.frozen
cargo test --offline --locked -- --ignored --test-threads=1 gateway_probe_is_refused_before_one_admitted_select > /out/test.txt 2> /out/test.stderr
printf '%s\n' "$?" > /out/test.exit
if ! cmp -s Cargo.lock /out/Cargo.lock.frozen; then
  echo "FAILED lock rewritten"
  exit 1
fi
if [ "$(cat /out/test.exit)" != "0" ]; then
  echo "FAILED test=$(cat /out/test.exit)"
  exit 1
fi
echo "DONE live"
exit 0
