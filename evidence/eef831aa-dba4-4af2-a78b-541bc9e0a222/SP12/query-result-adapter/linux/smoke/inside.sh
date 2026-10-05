#!/bin/bash
# Pre-implementation smoke of fixed commit c2db9c430b822a824a1062b6c3c7345924df7dd8.
# The query-result-adapter sources are not in this archive.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
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
echo "=== toolchain file absent ==="
if [ -f rust-toolchain.toml ]; then
  echo "repo toolchain file still active" >&2
  printf '%s\n' 2 > /evidence/toolchain-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/toolchain-absent.exit
if [ -f rust-toolchain.toml.repo ]; then
  echo "repo toolchain file was preserved beside the tree"
else
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

echo "=== pins and absence ==="
sha256sum \
  contracts/gateway/query-pg-cli-v1.md \
  crates/oo-gateway/src/bin/query-pg.rs \
  benchmarks/result-comparator/CONTRACT.md \
  benchmarks/result-comparator/Cargo.toml \
  benchmarks/result-comparator/Cargo.lock \
  Cargo.lock \
  > /evidence/archive-sha256.txt
if [ -e benchmarks/query-result-adapter ]; then
  echo "adapter present during smoke" >&2
  printf '%s\n' 2 > /evidence/adapter-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/adapter-absent.exit
if [ ! -f benchmarks/result-comparator/CONTRACT.md ]; then
  echo "comparator contract missing" >&2
  printf '%s\n' 2 > /evidence/comparator-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/comparator-present.exit
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

echo "=== cargo fetch comparator locked offline ==="
set +e
cargo fetch --manifest-path benchmarks/result-comparator/Cargo.toml --locked --offline \
  > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch comparator locked online ==="
  set +e
  cargo fetch --manifest-path benchmarks/result-comparator/Cargo.toml --locked \
    > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/comparator.exit
  exit "$fetch_exit"
fi

echo "=== cargo test result-comparator ==="
set +e
cargo test --manifest-path benchmarks/result-comparator/Cargo.toml --locked --offline -- --test-threads=1 \
  > /evidence/comparator.stdout 2> /evidence/comparator.stderr
comparator_exit=$?
set -e
printf '%s\n' "$comparator_exit" > /evidence/comparator.exit
echo "PHASE comparator_exit=$comparator_exit"
exit "$comparator_exit"
