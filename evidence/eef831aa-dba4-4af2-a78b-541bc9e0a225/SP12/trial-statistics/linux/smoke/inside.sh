#!/bin/bash
# Focused oo-core JSON smoke on the sealed c2db9c43 archive.
# The trial-statistics sources are not in this archive.
# Origin: a212 smoke inside 0ae42f942e075be4171646d373c49dcfe1029817
# evidence/eef831aa-dba4-4af2-a78b-541bc9e0a212/SP12/result-comparator/linux/smoke/inside.sh
# sha256 1f3d708b6fcab08576e0f06ec62c18daf808e301dd8883130c199c978103efb8
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

echo "=== pins ==="
sha256sum \
  crates/oo-core/src/json_scan.rs \
  crates/oo-core/src/limits.rs \
  crates/oo-core/src/lib.rs \
  crates/oo-core/Cargo.toml \
  Cargo.lock \
  > /evidence/archive-sha256.txt
if [ -e benchmarks/trial-statistics ]; then
  echo "trial-statistics present during smoke" >&2
  printf '%s\n' 2 > /evidence/statistics-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/statistics-absent.exit
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

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/json.exit
  exit "$fetch_exit"
fi

echo "=== cargo test oo-core json scanner ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-core --test frozen_corrections yaml_and_json_numbers_stay_lexical -- --test-threads=1 \
  > /evidence/json.stdout 2> /evidence/json.stderr
json_exit=$?
set -e
printf '%s\n' "$json_exit" > /evidence/json.exit
echo "PHASE json_exit=$json_exit"
exit "$json_exit"
