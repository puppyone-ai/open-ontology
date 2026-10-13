#!/bin/bash
# Linux Rust 1.94 build of the standalone driver workspace. Jobs stay at 1.
set -euo pipefail

cd /src
mkdir -p /evidence/bin
rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
rustup component list --installed | tee /evidence/rustup-components-before.txt
if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.txt 2>&1
fi
rustup component list --installed | tee /evidence/rustup-components.txt

cp Cargo.lock /evidence/Cargo.lock.before
set +e
cargo generate-lockfile > /evidence/cargo-generate-lockfile.txt 2>&1
gen_rc=$?
set -e
if [[ "$gen_rc" -ne 0 ]]; then
  printf 'generate-lockfile-failed %s\n' "$gen_rc" | tee /evidence/lock-resolution.txt
  exit "$gen_rc"
fi

resolve_ok=0
if bash /src/scripts/check-lock.sh /src/Cargo.lock /evidence/Cargo.lock.before /evidence/lock-checksums.txt; then
  printf 'generate-lockfile\n' | tee /evidence/lock-resolution.txt
  resolve_ok=1
else
  cp /src/Cargo.lock /evidence/Cargo.lock.rejected
  cp /evidence/Cargo.lock.before /src/Cargo.lock
  printf 'generate-lockfile-drifted\n' | tee /evidence/lock-resolution.txt
  cargo build --jobs 1 --bins > /evidence/cargo-build-lockfix.txt 2>&1
  bash /src/scripts/check-lock.sh /src/Cargo.lock /evidence/Cargo.lock.before /evidence/lock-checksums.txt \
    | tee /evidence/lock-checksums-after-minimal.txt
  printf 'minimal-lock-update\n' | tee -a /evidence/lock-resolution.txt
  resolve_ok=1
fi

if [[ "$resolve_ok" -ne 1 ]]; then
  exit 2
fi

cargo test --locked --jobs 1 --workspace > /evidence/cargo-test.txt 2>&1
cargo build --locked --jobs 1 --bins > /evidence/cargo-build.txt 2>&1
cargo clippy --locked --jobs 1 --all-targets --workspace -- -D warnings > /evidence/cargo-clippy.txt 2>&1

if cargo tree -p oo-mongodb-arrow-decode --locked --prefix none | awk '$1 == "mongodb" { found = 1 } END { exit found ? 0 : 1 }'; then
  printf 'decoder links mongodb\n' | tee /evidence/decoder-tree.txt
  exit 3
fi
cargo tree -p oo-mongodb-arrow-decode --locked --prefix none > /evidence/decoder-tree.txt
printf 'decoder_mongodb=absent\n' | tee -a /evidence/decoder-tree.txt

cp /target/debug/oo-mongodb-driver-probe /evidence/bin/oo-mongodb-driver-probe
cp /target/debug/oo-mongodb-arrow-decode /evidence/bin/oo-mongodb-arrow-decode
chmod 755 /evidence/bin/oo-mongodb-driver-probe /evidence/bin/oo-mongodb-arrow-decode
gcc -shared -fPIC -O2 -Wall -Wextra -Werror -o /evidence/bin/liboo-opmsg-names.so /src/scripts/opmsg-name-log.c -ldl
chmod 755 /evidence/bin/liboo-opmsg-names.so
cp /src/Cargo.lock /evidence/Cargo.lock
printf 'build-ok\n' | tee /evidence/build-result.txt
