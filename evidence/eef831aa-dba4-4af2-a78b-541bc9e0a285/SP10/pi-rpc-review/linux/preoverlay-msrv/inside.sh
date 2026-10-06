#!/bin/bash
# Pre-overlay MSRV smoke. Immutable archive. Network none. No clippy in this container.
# Rust 1.94.0 runs the twelve boundary tests, then the review oracle reads that stdout.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export PATH=/usr/local/cargo/bin:/usr/local/bin:${PATH}
export CARGO_TERM_COLOR=never
cd /work
worst=0
keep() {
  if [ "$1" -ne 0 ] && [ "$worst" -eq 0 ]; then
    worst=$1
  fi
}
echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
uname -a
cat /etc/os-release
id
rustup show > /proof/toolchain.txt
rustc -vV >> /proof/toolchain.txt
echo "=== compile oracles ==="
set +e
rustc --edition 2024 -D warnings /tools/review_oracle.rs -o /target/review_oracle > /proof/oracle-rustc.stdout 2> /proof/oracle-rustc.stderr
oracle_build=$?
rustc --edition 2024 -D warnings /tools/sha256_manifest.rs -o /target/sha256_manifest > /proof/manifest-rustc.stdout 2> /proof/manifest-rustc.stderr
manifest_build=$?
set -e
printf '%s\n' "$oracle_build" > /proof/oracle-rustc.exit
printf '%s\n' "$manifest_build" > /proof/manifest-rustc.exit
echo "PHASE oracle_build=$oracle_build manifest_build=$manifest_build"
keep "$oracle_build"
keep "$manifest_build"
if [ "$oracle_build" -eq 0 ]; then
  set +e
  /target/review_oracle spike /work > /proof/spike-oracle.stdout 2> /proof/spike-oracle.stderr
  spike_exit=$?
  /target/review_oracle historical /historical/a279/clippy.exit /historical/a282/a279-compare.stdout > /proof/historical-oracle.stdout 2> /proof/historical-oracle.stderr
  historical_exit=$?
  /target/review_oracle same /origins/a204-sha256_manifest.rs /tools/sha256_manifest.rs > /proof/tool-same.stdout 2> /proof/tool-same.stderr
  same_exit=$?
  set -e
  printf '%s\n' "$spike_exit" > /proof/spike-oracle.exit
  printf '%s\n' "$historical_exit" > /proof/historical-oracle.exit
  printf '%s\n' "$same_exit" > /proof/tool-same.exit
  echo "PHASE spike=$spike_exit historical=$historical_exit same=$same_exit"
  keep "$spike_exit"
  keep "$historical_exit"
  keep "$same_exit"
fi
if [ "$manifest_build" -eq 0 ]; then
  set +e
  /target/sha256_manifest /work /proof/source-spike.sha256 > /proof/source-spike.stdout 2> /proof/source-spike.stderr
  source_hash_exit=$?
  /target/sha256_manifest /origins /proof/origin-files.sha256 > /proof/origin-files.stdout 2> /proof/origin-files.stderr
  origin_hash_exit=$?
  set -e
  printf '%s\n' "$source_hash_exit" > /proof/source-spike.exit
  printf '%s\n' "$origin_hash_exit" > /proof/origin-files.exit
  echo "PHASE source_hash=$source_hash_exit origin_hash=$origin_hash_exit"
  keep "$source_hash_exit"
  keep "$origin_hash_exit"
fi
echo "=== boundary tests ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /proof/test.stdout 2> /proof/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /proof/test.exit
echo "PHASE test_exit=$test_exit"
keep "$test_exit"
if [ "$oracle_build" -eq 0 ]; then
  set +e
  /target/review_oracle boundary /proof/test.stdout > /proof/boundary-oracle.stdout 2> /proof/boundary-oracle.stderr
  boundary_exit=$?
  set -e
  printf '%s\n' "$boundary_exit" > /proof/boundary-oracle.exit
  echo "PHASE boundary_exit=$boundary_exit"
  keep "$boundary_exit"
fi
echo "PHASE worst=$worst"
printf '%s\n' "$worst" > /proof/worst.exit
exit "$worst"
