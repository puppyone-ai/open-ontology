#!/bin/bash
# Linux Rust 1.94 build for the OpenSearch driver and the preview admission CLI.
set -u
export CARGO_BUILD_JOBS=1
export RUSTUP_TOOLCHAIN=1.94.0
export PATH="/usr/local/cargo/bin:${PATH}"
mkdir -p /evidence/build /target/driver /target/product
cd /src
rustup run 1.94.0 rustc -vV > /evidence/build/rustc-vv.txt
if ! grep -q '^release: 1.94.0$' /evidence/build/rustc-vv.txt; then
  printf '91\n' > /evidence/build/build.exit
  exit 91
fi
set +e
if [ ! -f Cargo.lock ]; then
  rustup run 1.94.0 cargo generate-lockfile --offline > /evidence/build/generate-lock.stdout 2> /evidence/build/generate-lock.stderr
  gen=$?
  if [ "$gen" != "0" ]; then
    rustup run 1.94.0 cargo generate-lockfile > /evidence/build/generate-lock-online.stdout 2> /evidence/build/generate-lock-online.stderr
    gen=$?
    printf '%s\n' "$gen" > /evidence/build/generate-lock-online.exit
  fi
  printf '%s\n' "$gen" > /evidence/build/generate-lock.exit
  if [ "$gen" != "0" ]; then
    printf '%s\n' "$gen" > /evidence/build/build.exit
    exit "$gen"
  fi
fi
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
rustup run 1.94.0 cargo fetch --locked > /evidence/build/fetch.stdout 2> /evidence/build/fetch.stderr
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
cd /preview
if [ ! -f Cargo.lock ]; then
  rustup run 1.94.0 cargo generate-lockfile > /evidence/build/preview-lock.stdout 2> /evidence/build/preview-lock.stderr
  pgen=$?
  printf '%s\n' "$pgen" > /evidence/build/preview-lock.exit
  if [ "$pgen" != "0" ]; then
    printf '%s\n' "$pgen" > /evidence/build/preview.exit
    printf '%s\n' "$pgen" > /evidence/build/build.exit
    exit "$pgen"
  fi
fi
export CARGO_TARGET_DIR=/target/product
rustup run 1.94.0 cargo fetch --locked > /evidence/build/preview-fetch.stdout 2> /evidence/build/preview-fetch.stderr
pf=$?
printf '%s\n' "$pf" > /evidence/build/preview-fetch.exit
if [ "$pf" != "0" ]; then
  printf '%s\n' "$pf" > /evidence/build/preview.exit
  printf '%s\n' "$pf" > /evidence/build/build.exit
  exit "$pf"
fi
rustup run 1.94.0 cargo test --locked --offline --jobs 1 --all-targets \
  > /evidence/build/preview-test.stdout 2> /evidence/build/preview-test.stderr
pt=$?
printf '%s\n' "$pt" > /evidence/build/preview-test.exit
if [ "$pt" != "0" ]; then
  printf '%s\n' "$pt" > /evidence/build/preview.exit
  printf '%s\n' "$pt" > /evidence/build/build.exit
  exit "$pt"
fi
rustup run 1.94.0 cargo clippy --locked --offline --jobs 1 --all-targets -- -D warnings \
  > /evidence/build/preview-clippy.stdout 2> /evidence/build/preview-clippy.stderr
pc=$?
printf '%s\n' "$pc" > /evidence/build/preview-clippy.exit
if [ "$pc" != "0" ]; then
  printf '%s\n' "$pc" > /evidence/build/preview.exit
  printf '%s\n' "$pc" > /evidence/build/build.exit
  exit "$pc"
fi
rustup run 1.94.0 cargo build --locked --offline --jobs 1 \
  > /evidence/build/preview-build.stdout 2> /evidence/build/preview-build.stderr
pb=$?
printf '%s\n' "$pb" > /evidence/build/preview.exit
if [ "$pb" != "0" ]; then
  printf '%s\n' "$pb" > /evidence/build/build.exit
  exit "$pb"
fi
cp /target/product/debug/opensearch-preview /evidence/bin/
sha256sum /evidence/bin/opensearch-preview >> /evidence/build/binaries.sha256
printf '0\n' > /evidence/build/build.exit
exit 0
