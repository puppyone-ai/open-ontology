#!/bin/bash
# Clippy and tests inside the pinned Rust image. No login shell.
set -eu
export PATH="/usr/local/cargo/bin:${PATH}"
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME=/cargo
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
mkdir -p /out
rustc --version
cargo --version
uname -m
cd /build/tools/release-coordinator
awk '
  FNR==NR { if ($0 ~ /checksum = /) root[$0]=1; next }
  $0 ~ /checksum = / && !($0 in root) { print "lock_checksum_not_in_root " $0; bad=1 }
  END { if (bad) exit 1; print "registry_checksums_subset_of_root=yes" }
' /immutable/Cargo.lock Cargo.lock | tee /out/lock-subset-check.txt
cargo fetch --locked
rustup component list --toolchain 1.94.0 > /out/components-before.txt
rustup component add clippy --toolchain 1.94.0
export CARGO_NET_OFFLINE=true
export CARGO_TARGET_DIR=/target/product
set +e
cargo clippy --locked --offline --all-targets -- -D warnings
printf '%s\n' "$?" > /out/author-clippy.exit
cargo test --locked --offline -- --test-threads=1
printf '%s\n' "$?" > /out/author-test.exit
cargo build --locked --offline --bin release-coordinator
printf '%s\n' "$?" > /out/author-build.exit
set -e
test -x /target/product/debug/release-coordinator
if [ "$(cat /out/author-clippy.exit)" != 0 ] || [ "$(cat /out/author-test.exit)" != 0 ] || [ "$(cat /out/author-build.exit)" != 0 ]; then
  echo ARCHIVE_BASELINE_FAILED
else
  echo ARCHIVE_OK
fi
export CARGO_TARGET_DIR=/target/review
cd /review/tools/release-coordinator
set +e
cargo clippy --locked --offline --all-targets -- -D warnings
printf '%s\n' "$?" > /out/review-clippy.exit
cargo test --locked --offline -- --test-threads=1 review_
printf '%s\n' "$?" > /out/review-test.exit
set -e
if [ "$(cat /out/review-clippy.exit)" != 0 ] || [ "$(cat /out/review-test.exit)" != 0 ]; then
  echo REVIEW_FAILED
  exit 1
fi
echo REVIEW_OK
if [ "$(cat /out/author-clippy.exit)" != 0 ] || [ "$(cat /out/author-test.exit)" != 0 ] || [ "$(cat /out/author-build.exit)" != 0 ]; then
  exit 1
fi
echo BUILD_OK
