#!/bin/bash
# Linux 1.94 driver build. Pins transitive crates down to the baseline lock.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export CARGO_TARGET_DIR=/tmp/target
export GIT_TERMINAL_PROMPT=0
cd /src
rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
grep -q '^rustc 1.94.0' /evidence/rustc-version.txt

set +e
cargo generate-lockfile --offline > /evidence/lockfile.stdout 2> /evidence/lockfile.stderr
lock_ec=$?
if [[ "$lock_ec" == "0" ]]; then
  echo offline > /evidence/lockfile.mode
else
  echo online > /evidence/lockfile.mode
  cargo generate-lockfile > /evidence/lockfile.stdout 2> /evidence/lockfile.stderr
  lock_ec=$?
fi
echo "$lock_ec" > /evidence/lockfile.exit
set -e
if [[ "$lock_ec" != "0" ]]; then
  exit "$lock_ec"
fi
cp Cargo.lock /evidence/Cargo.lock.unpinned

: > /evidence/pin.stdout
: > /evidence/pin.stderr
status=0
pin_one() {
  echo "pin $1 $2" >> /evidence/pin.stdout
  if ! cargo update -p "$1" --precise "$2" >> /evidence/pin.stdout 2>> /evidence/pin.stderr; then
    echo "pin-failed $1 $2" >> /evidence/pin.stdout
    status=1
  fi
}
pin_one cc 1.4.7
pin_one crypto-common@0.1.7 0.1.6
pin_one find-msvc-tools 0.1.13
pin_one generic-array 0.14.9
pin_one js-sys 0.3.105
pin_one libc 0.2.189
pin_one mio 1.2.3
pin_one smallvec 1.16.1
pin_one syn@3.0.7 3.0.6
pin_one wasm-bindgen 0.2.128
pin_one wasm-bindgen-macro 0.2.128
pin_one wasm-bindgen-macro-support 0.2.128
pin_one wasm-bindgen-shared 0.2.128
pin_one yoke-derive 0.8.2
pin_one zerocopy 0.8.57
pin_one zerocopy-derive 0.8.57
pin_one zeroize 1.9.0
echo "$status" > /evidence/pin.exit
cp Cargo.lock /evidence/Cargo.lock

set +e
bash /src/scripts/check-lock.bash > /evidence/check-lock.stdout 2> /evidence/check-lock.stderr
check_ec=$?
echo "$check_ec" > /evidence/check-lock.exit
set -e
if [[ "$check_ec" != "0" ]]; then
  exit "$check_ec"
fi

run_cargo() {
  local name="$1"
  shift
  set +e
  cargo "$@" --offline > "/evidence/${name}.stdout" 2> "/evidence/${name}.stderr"
  local ec=$?
  if [[ "$ec" != "0" ]]; then
    cargo "$@" > "/evidence/${name}.stdout" 2> "/evidence/${name}.stderr"
    ec=$?
  fi
  echo "$ec" > "/evidence/${name}.exit"
  set -e
  if [[ "$ec" != "0" ]]; then
    exit "$ec"
  fi
}

run_cargo test test --locked --jobs 1 --workspace --all-targets
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
comp_ec=$?
echo "$comp_ec" > /evidence/clippy-component.exit
if [[ "$comp_ec" == "0" ]]; then
  rustup run 1.94.0 cargo clippy --version > /evidence/clippy-version.txt 2>> /evidence/clippy-component.stderr
fi
set -e
if [[ "$comp_ec" != "0" ]]; then
  exit "$comp_ec"
fi
run_cargo clippy clippy --locked --jobs 1 --workspace --all-targets -- -D warnings
run_cargo build build --locked --jobs 1 --workspace
mkdir -p /evidence/bin
cp /tmp/target/debug/doris-reader /tmp/target/debug/doris-decode /evidence/bin/
cp Cargo.lock /evidence/Cargo.lock
echo driver-build-ok > /evidence/build-result.txt
