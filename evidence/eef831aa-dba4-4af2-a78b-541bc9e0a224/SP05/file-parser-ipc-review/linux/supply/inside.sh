#!/bin/bash
# Re-run only the supply audit after preoverlay supply-rustc exit 1.
# The child smoke and clippy stay in linux/preoverlay. This does not overwrite them.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src
mkdir -p /target/review-bin /evidence/supply

echo "=== identity ==="
rustc -vV
release=$(rustc -vV | awk '/^release:/{print $2}')
printf '%s\n' "$release" > /evidence/rustc-release.txt
if [ "$release" != "1.94.0" ]; then
  echo "toolchain is ${release}" >&2
  printf '%s\n' 2 > /evidence/toolchain.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/toolchain.exit
if ! cmp -s Cargo.lock /blobs/Cargo.lock; then
  echo "lock drifted" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
echo "PHASE pins=0"

echo "=== supply rustc ==="
rustc --edition 2024 -D warnings /tools/supply_audit.rs -o /target/review-bin/supply_audit \
  > /evidence/supply-rustc.stdout 2> /evidence/supply-rustc.stderr
supply_rustc=$?
printf '%s\n' "$supply_rustc" > /evidence/supply-rustc.exit
echo "PHASE supply_rustc=$supply_rustc"
if [ "$supply_rustc" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/supply-curl.exit
  printf '%s\n' not_run > /evidence/supply.exit
  exit "$supply_rustc"
fi

echo "=== supply index and api ==="
curl_one() {
  url=$1
  out=$2
  tag=$3
  /usr/bin/curl -fsSL --compressed --retry 2 --retry-delay 2 --max-time 60 \
    -A 'oo-a224-file-parser-ipc-review' \
    -o "$out" "$url" > "/evidence/supply/${tag}.stdout" 2> "/evidence/supply/${tag}.stderr"
  status=$?
  printf '%s\n' "$status" > "/evidence/supply/${tag}.exit"
  echo "PHASE curl_${tag}=$status"
  return "$status"
}
curl_fail=0
curl_one https://index.crates.io/ar/ro/arrow-ipc /evidence/supply/arrow-ipc.index arrow-ipc-index || curl_fail=$?
curl_one https://index.crates.io/ar/ro/arrow-cmp /evidence/supply/arrow-cmp.index arrow-cmp-index || curl_fail=$?
curl_one https://index.crates.io/ar/ro/arrow-select /evidence/supply/arrow-select.index arrow-select-index || curl_fail=$?
curl_one https://index.crates.io/fl/at/flatbuffers /evidence/supply/flatbuffers.index flatbuffers-index || curl_fail=$?
curl_one https://crates.io/api/v1/crates/arrow-ipc/60.0.0 /evidence/supply/arrow-ipc.api.json arrow-ipc-api || curl_fail=$?
curl_one https://crates.io/api/v1/crates/arrow-cmp/60.0.0 /evidence/supply/arrow-cmp.api.json arrow-cmp-api || curl_fail=$?
curl_one https://crates.io/api/v1/crates/arrow-select/60.0.0 /evidence/supply/arrow-select.api.json arrow-select-api || curl_fail=$?
curl_one https://crates.io/api/v1/crates/flatbuffers/25.12.19 /evidence/supply/flatbuffers.api.json flatbuffers-api || curl_fail=$?
printf '%s\n' "$curl_fail" > /evidence/supply-curl.exit
date -u +%Y-%m-%dT%H:%M:%SZ > /evidence/supply/clock.txt
echo "PHASE supply_curl=$curl_fail"

echo "=== supply audit ==="
/target/review-bin/supply_audit /src/Cargo.lock /locks/parent.lock /cargo-home /evidence/supply /evidence/supply/clock.txt \
  > /evidence/supply.stdout 2> /evidence/supply.stderr
supply_exit=$?
printf '%s\n' "$supply_exit" > /evidence/supply.exit
echo "PHASE supply_exit=$supply_exit"
exit "$supply_exit"
