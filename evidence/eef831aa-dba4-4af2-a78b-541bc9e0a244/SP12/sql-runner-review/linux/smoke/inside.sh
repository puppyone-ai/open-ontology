#!/bin/bash
# Focused sql-runner tests on the sealed 8f533140 archive, before the live proof.
# Origin shape: a204 preoverlay and a231 smoke. Delta: this commit contains sql-runner.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_PROFILE_DEV_DEBUG=0
cd /src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
if [ -f rust-toolchain.toml ]; then
  echo "repo toolchain file still active" >&2
  printf '%s\n' 2 > /evidence/toolchain-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/toolchain-absent.exit
if [ ! -f rust-toolchain.toml.repo ]; then
  echo "repo toolchain file missing from archive copy" >&2
  printf '%s\n' 2 > /evidence/toolchain-absent.exit
  exit 2
fi
rustc_version=$(rustc -vV | awk '/^release:/{print $2}')
printf '%s\n' "$rustc_version" > /evidence/rustc-release.txt
if [ "$rustc_version" != "1.94.0" ]; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/rustc-release.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/rustc-release.exit

if [ ! -f benchmarks/sql-runner/Cargo.toml ] || [ ! -f benchmarks/sql-runner/src/lib.rs ]; then
  echo "sql-runner missing from the sealed archive" >&2
  printf '%s\n' 2 > /evidence/runner-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/runner-present.exit
if [ -e /src/evidence ] || [ -e benchmarks/sql-runner/tests/a244_review.rs ]; then
  echo "review harness is inside the sealed archive" >&2
  printf '%s\n' 2 > /evidence/harness-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/harness-absent.exit

echo "=== pins ==="
sha256sum \
  benchmarks/sql-runner/Cargo.toml \
  benchmarks/sql-runner/Cargo.lock \
  benchmarks/sql-runner/src/lib.rs \
  benchmarks/sql-runner/src/drive.rs \
  benchmarks/sql-runner/CONTRACT.md \
  > /evidence/archive-sha256.txt
awk '
  $0=="name = \"serde\"" {p=1}
  $0=="name = \"serde_json\"" {p=2}
  p==1 && $0 ~ /^version = / {print "serde",$0}
  p==1 && $0 ~ /^checksum = / {print "serde",$0; p=0}
  p==2 && $0 ~ /^version = / {print "serde_json",$0}
  p==2 && $0 ~ /^checksum = / {print "serde_json",$0; p=0}
' benchmarks/sql-runner/Cargo.lock > /evidence/lock-pins.txt
if ! grep -q 'serde version = "1.0.229"' /evidence/lock-pins.txt \
  || ! grep -q 'serde checksum = "4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba"' /evidence/lock-pins.txt \
  || ! grep -q 'serde_json version = "1.0.151"' /evidence/lock-pins.txt \
  || ! grep -q 'serde_json checksum = "c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14"' /evidence/lock-pins.txt
then
  echo "lock pins drifted" >&2
  printf '%s\n' 2 > /evidence/lock-pins.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-pins.exit

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path benchmarks/sql-runner/Cargo.toml --locked --offline \
  > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path benchmarks/sql-runner/Cargo.toml --locked \
    > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  exit "$fetch_exit"
fi

echo "=== cargo test sql-runner ==="
set +e
cargo test --manifest-path benchmarks/sql-runner/Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
exit "$test_exit"
