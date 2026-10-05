#!/bin/bash
# Pre-overlay Linux arm64 Rust 1.94 smoke of frozen 6bd2455849da2527973c136968d4549ecd98d770.
# The archive must not contain the review harness or the parent export.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

mark_missing() {
  for name in "$@"; do
    if [ ! -f "/evidence/${name}" ]; then
      printf '%s\n' not_run > "/evidence/${name}"
    fi
  done
}

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
echo "=== toolchain file ==="
cat rust-toolchain.toml || true
release=$(rustc -vV | awk '/^release:/{print $2}')
printf '%s\n' "$release" > /evidence/rustc-release.txt
if [ "$release" != "1.94.0" ]; then
  echo "toolchain is ${release}" >&2
  printf '%s\n' 2 > /evidence/toolchain.exit
  mark_missing blob-cmp.exit overlay-absent.exit parent-absent.exit origin-rustc.exit origin.exit \
    fetch-offline.exit fetch.exit test.exit supply-curl.exit supply-rustc.exit supply.exit \
    clippy-component.exit clippy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/toolchain.exit
echo "PHASE toolchain=1.94.0"

echo "=== source pins ==="
sha256sum \
  crates/oo-connectors/src/bin/oo-file-parser-worker.rs \
  crates/oo-connectors/tests/file_parser_worker.rs \
  contracts/connectors/file-parser-worker-v1.md \
  crates/oo-connectors/Cargo.toml \
  Cargo.lock \
  crates/oo-connectors/src/delimited.rs \
  crates/oo-connectors/src/jsonl.rs \
  crates/oo-connectors/src/lib.rs \
  > /evidence/archive-sha256.txt
if ! cmp -s crates/oo-connectors/src/bin/oo-file-parser-worker.rs /blobs/oo-file-parser-worker.rs \
  || ! cmp -s crates/oo-connectors/tests/file_parser_worker.rs /blobs/file_parser_worker.rs \
  || ! cmp -s contracts/connectors/file-parser-worker-v1.md /blobs/file-parser-worker-v1.md \
  || ! cmp -s crates/oo-connectors/Cargo.toml /blobs/oo-connectors-Cargo.toml \
  || ! cmp -s Cargo.lock /blobs/Cargo.lock \
  || ! cmp -s crates/oo-connectors/src/delimited.rs /blobs/delimited.rs \
  || ! cmp -s crates/oo-connectors/src/jsonl.rs /blobs/jsonl.rs \
  || ! cmp -s crates/oo-connectors/src/lib.rs /blobs/lib.rs
then
  echo "archive drifted from the saved blobs" >&2
  printf '%s\n' 2 > /evidence/blob-cmp.exit
  mark_missing overlay-absent.exit parent-absent.exit origin-rustc.exit origin.exit \
    fetch-offline.exit fetch.exit test.exit supply-curl.exit supply-rustc.exit supply.exit \
    clippy-component.exit clippy.exit hash-compare.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -f crates/oo-connectors/tests/file_parser_worker_a224_review.rs ]; then
  echo "overlay present during preoverlay" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  mark_missing parent-absent.exit origin-rustc.exit origin.exit fetch-offline.exit fetch.exit \
    test.exit supply-curl.exit supply-rustc.exit supply.exit clippy-component.exit clippy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
if [ -e crates/oo-connectors/src/file_worker_parent.rs ] || grep -q file_worker_parent crates/oo-connectors/src/lib.rs; then
  echo "parent export is in the archive" >&2
  printf '%s\n' 2 > /evidence/parent-absent.exit
  mark_missing origin-rustc.exit origin.exit fetch-offline.exit fetch.exit test.exit \
    supply-curl.exit supply-rustc.exit supply.exit clippy-component.exit clippy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/parent-absent.exit
if ! sha256sum -c /evidence/expect-sha256.txt > /evidence/hash-compare.txt 2>&1; then
  echo "archive hash mismatch" >&2
  printf '%s\n' 2 > /evidence/hash-compare.exit
  mark_missing origin-rustc.exit origin.exit fetch-offline.exit fetch.exit test.exit \
    supply-curl.exit supply-rustc.exit supply.exit clippy-component.exit clippy.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/hash-compare.exit
echo "PHASE pins=0"

echo "=== origin recipe hashes ==="
mkdir -p /target/review-bin /evidence/supply
rustc --edition 2024 -D warnings /tools/sha256_files.rs -o /target/review-bin/sha256_files \
  > /evidence/origin-rustc.stdout 2> /evidence/origin-rustc.stderr
origin_rustc=$?
printf '%s\n' "$origin_rustc" > /evidence/origin-rustc.exit
echo "PHASE origin_rustc=$origin_rustc"
if [ "$origin_rustc" -ne 0 ]; then
  mark_missing origin.exit fetch-offline.exit fetch.exit test.exit supply-curl.exit \
    supply-rustc.exit supply.exit clippy-component.exit clippy.exit
  exit "$origin_rustc"
fi
/target/review-bin/sha256_files check-list /recipes/origin.list \
  > /evidence/origin.stdout 2> /evidence/origin.stderr
origin_exit=$?
printf '%s\n' "$origin_exit" > /evidence/origin.exit
echo "PHASE origin_exit=$origin_exit"
if [ "$origin_exit" -ne 0 ]; then
  mark_missing fetch-offline.exit fetch.exit test.exit supply-curl.exit supply-rustc.exit \
    supply.exit clippy-component.exit clippy.exit
  exit "$origin_exit"
fi

echo "=== cargo fetch locked offline ==="
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  mark_missing test.exit supply-curl.exit supply-rustc.exit supply.exit clippy-component.exit clippy.exit
  exit "$fetch_exit"
fi

echo "=== cargo test file_parser_worker ==="
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test file_parser_worker -- --nocapture --test-threads=8 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

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

echo "=== supply audit ==="
rustc --edition 2024 -D warnings /tools/supply_audit.rs -o /target/review-bin/supply_audit \
  > /evidence/supply-rustc.stdout 2> /evidence/supply-rustc.stderr
supply_rustc=$?
printf '%s\n' "$supply_rustc" > /evidence/supply-rustc.exit
echo "PHASE supply_rustc=$supply_rustc"
if [ "$supply_rustc" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/supply.exit
  supply_exit=$supply_rustc
else
  /target/review-bin/supply_audit /src/Cargo.lock /locks/parent.lock /cargo-home /evidence/supply /evidence/supply/clock.txt \
    > /evidence/supply.stdout 2> /evidence/supply.stderr
  supply_exit=$?
  printf '%s\n' "$supply_exit" > /evidence/supply.exit
fi
echo "PHASE supply_exit=$supply_exit"

echo "=== rustup component add clippy ==="
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  clippy_exit=$component_exit
else
  echo "=== cargo clippy file parser worker ==="
  cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors \
    --bin oo-file-parser-worker --test file_parser_worker -- -D warnings \
    > /evidence/clippy.stdout 2> /evidence/clippy.stderr
  clippy_exit=$?
  printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
fi
echo "PHASE clippy_exit=$clippy_exit"

worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$supply_exit" -ne 0 ]; then worst=$supply_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
