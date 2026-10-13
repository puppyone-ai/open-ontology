#!/bin/bash
# Pin the transitive crates that floated off the MySQL recipe lock. One --precise per invocation.
set -u
export HOME=/tmp
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
export PATH="/usr/local/cargo/bin:${PATH}"
cd /work
mkdir -p /evidence/build
: > /evidence/build/pin-lock.stdout
: > /evidence/build/pin-lock.stderr
status=0
pin_one() {
  echo "pin $1 $2" >> /evidence/build/pin-lock.stdout
  if ! cargo update -p "$1" --precise "$2" >> /evidence/build/pin-lock.stdout 2>> /evidence/build/pin-lock.stderr; then
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
pin_one wasm-bindgen 0.2.128
pin_one wasm-bindgen-macro 0.2.128
pin_one wasm-bindgen-macro-support 0.2.128
pin_one wasm-bindgen-shared 0.2.128
pin_one yoke-derive 0.8.2
pin_one zerocopy 0.8.57
pin_one zerocopy-derive 0.8.57
pin_one zeroize 1.9.0
printf '%s\n' "$status" > /evidence/build/pin-lock.exit
exit "$status"
