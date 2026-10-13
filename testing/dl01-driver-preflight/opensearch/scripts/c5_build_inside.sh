#!/bin/bash
# Linux Rust 1.94 test, clippy, and build for the C5 reader. Preview is not rebuilt.
set -u
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
export PATH="/usr/local/cargo/bin:${PATH}"
mkdir -p /evidence/build /target/driver
cd /src
rustup run 1.94.0 rustc -vV > /evidence/build/rustc-vv.txt
if ! grep -q '^release: 1.94.0$' /evidence/build/rustc-vv.txt; then
  printf '91\n' > /evidence/build/build.exit
  exit 91
fi
set +e
pairs() {
  awk '
    /^\[\[package\]\]/ { if (name != "") print name "\t" ver; name=""; ver="" }
    /^name = / { gsub(/"/, "", $3); name=$3 }
    /^version = / { gsub(/"/, "", $3); ver=$3 }
    END { if (name != "") print name "\t" ver }
  ' "$1"
}
pairs /base/Cargo.lock | sort -u > /tmp/base.pairs
: > /evidence/build/lock-report.txt
drift=0
while IFS=$'\t' read -r name ver; do
  case "$name" in
    os-reader|os-decode|os-judge|opensearch-preview) continue ;;
  esac
  if grep -qx "${name}	${ver}" /tmp/base.pairs; then
    continue
  fi
  if grep -q "^${name}	" /tmp/base.pairs; then
    printf 'DRIFT %s %s\n' "$name" "$ver" >> /evidence/build/lock-report.txt
    drift=1
  else
    printf 'NEW %s %s\n' "$name" "$ver" >> /evidence/build/lock-report.txt
  fi
done < <(pairs /src/Cargo.lock)
if [ "$drift" != "0" ]; then
  lock=37
else
  lock=0
fi
printf '%s\n' "$lock" > /evidence/build/lock.exit
if [ "$lock" != "0" ]; then
  printf '%s\n' "$lock" > /evidence/build/build.exit
  exit "$lock"
fi
export CARGO_TARGET_DIR=/target/driver
rustup run 1.94.0 cargo fetch --locked --offline > /evidence/build/fetch.stdout 2> /evidence/build/fetch.stderr
fetch=$?
printf '%s\n' "$fetch" > /evidence/build/fetch.exit
if [ "$fetch" != "0" ]; then
  printf '%s\n' "$fetch" > /evidence/build/build.exit
  exit "$fetch"
fi
rustup run 1.94.0 cargo test --locked --offline --jobs 1 --workspace --all-targets \
  > /evidence/build/test.stdout 2> /evidence/build/test.stderr
test_code=$?
printf '%s\n' "$test_code" > /evidence/build/test.exit
if [ "$test_code" != "0" ]; then
  printf '%s\n' "$test_code" > /evidence/build/build.exit
  exit "$test_code"
fi
if ! rustup run 1.94.0 cargo clippy --version >/dev/null 2>&1; then
  rustup component add clippy --toolchain 1.94.0 \
    > /evidence/build/clippy-component.stdout 2> /evidence/build/clippy-component.stderr
  component=$?
  printf '%s\n' "$component" > /evidence/build/clippy-component.exit
  if [ "$component" != "0" ]; then
    printf '%s\n' "$component" > /evidence/build/build.exit
    exit "$component"
  fi
fi
rustup run 1.94.0 cargo clippy --locked --offline --jobs 1 --workspace --all-targets -- -D warnings \
  > /evidence/build/clippy.stdout 2> /evidence/build/clippy.stderr
clippy_code=$?
printf '%s\n' "$clippy_code" > /evidence/build/clippy.exit
if [ "$clippy_code" != "0" ]; then
  printf '%s\n' "$clippy_code" > /evidence/build/build.exit
  exit "$clippy_code"
fi
rustup run 1.94.0 cargo build --locked --offline --jobs 1 --workspace \
  > /evidence/build/build.stdout 2> /evidence/build/build.stderr
build_code=$?
printf '%s\n' "$build_code" > /evidence/build/driver-build.exit
if [ "$build_code" != "0" ]; then
  printf '%s\n' "$build_code" > /evidence/build/build.exit
  exit "$build_code"
fi
mkdir -p /evidence/bin
cp /target/driver/debug/os-reader /target/driver/debug/os-decode /target/driver/debug/os-judge /evidence/bin/
sha256sum /evidence/bin/os-reader /evidence/bin/os-decode /evidence/bin/os-judge > /evidence/build/binaries.sha256
printf '0\n' > /evidence/build/build.exit
exit 0
