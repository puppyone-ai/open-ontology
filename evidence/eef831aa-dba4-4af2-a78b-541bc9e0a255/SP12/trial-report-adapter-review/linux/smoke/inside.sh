#!/bin/bash
# Focused MSRV smoke of the sealed adapter bounds tests.
# The review harness is not mounted. No PostgreSQL.
# Origin: a204 preoverlay inside, owner a255, fixed 8e7925d7, bounds test instead of a product CLI.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

echo "=== identity ==="
rustc -vV
cargo -V
rustup show
uname -a
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

if [ ! -f benchmarks/trial-report-adapter/Cargo.lock ]; then
  echo "sealed adapter lock is absent" >&2
  printf '%s\n' 2 > /evidence/adapter-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/adapter-present.exit
if grep -R -q 'A255-PASS' benchmarks/trial-report-adapter; then
  echo "review marker leaked into the sealed adapter" >&2
  printf '%s\n' 2 > /evidence/harness-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/harness-absent.exit

echo "=== pins ==="
awk '
  $0=="name = \"serde\"" {p=1}
  $0=="name = \"serde_json\"" {p=2}
  p==1 && $0 ~ /^version = / {print "serde",$0}
  p==1 && $0 ~ /^checksum = / {print "serde",$0; p=0}
  p==2 && $0 ~ /^version = / {print "serde_json",$0}
  p==2 && $0 ~ /^checksum = / {print "serde_json",$0; p=0}
' Cargo.lock > /evidence/lock-pins.txt
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

fetch_one() {
  local name="$1"
  local manifest="$2"
  echo "=== cargo fetch $name offline ==="
  set +e
  cargo fetch --manifest-path "$manifest" --locked --offline > "/evidence/${name}-fetch-offline.stdout" 2> "/evidence/${name}-fetch-offline.stderr"
  local off=$?
  set -e
  printf '%s\n' "$off" > "/evidence/${name}-fetch-offline.exit"
  echo "PHASE ${name}_offline_fetch_exit=$off"
  if [ "$off" -ne 0 ]; then
    echo "=== cargo fetch $name online ==="
    set +e
    cargo fetch --manifest-path "$manifest" --locked > "/evidence/${name}-fetch.stdout" 2> "/evidence/${name}-fetch.stderr"
    local on=$?
    set -e
    printf '%s\n' "$on" > "/evidence/${name}-fetch.exit"
    echo "PHASE ${name}_fetch_exit=$on"
    return "$on"
  fi
  cp "/evidence/${name}-fetch-offline.stdout" "/evidence/${name}-fetch.stdout"
  cp "/evidence/${name}-fetch-offline.stderr" "/evidence/${name}-fetch.stderr"
  printf '%s\n' 0 > "/evidence/${name}-fetch.exit"
  echo "PHASE ${name}_fetch_exit=0"
}

fetch_one adapter benchmarks/trial-report-adapter/Cargo.toml
fetch_one sql-runner benchmarks/sql-runner/Cargo.toml
fetch_one trial-statistics benchmarks/trial-statistics/Cargo.toml

echo "=== cargo test bounds ==="
set +e
cargo test --manifest-path benchmarks/trial-report-adapter/Cargo.toml --locked --offline --test bounds -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE bounds_exit=$test_exit"
exit "$test_exit"
