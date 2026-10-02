#!/bin/bash
# Runs inside the one a105 container. Cargo commands only. The Rust tests are the checker.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=2
export HOME=/work/home
export TMPDIR=/tmp
export GIT_TERMINAL_PROMPT=0
mkdir -p "$HOME"

fail() {
  printf 'FAILED %s\n' "$1" | tee /work/status.txt
  exit 1
}

{
  echo "uname=$(uname -a)"
  rustup toolchain list || true
} > /work/toolchains.txt

if ! rustup run 1.94.0 rustc --version > /work/rustc-version.txt 2> /work/rustc-version.err; then
  rustup toolchain install 1.94.0 --profile minimal --component clippy > /work/toolchain-install.out 2> /work/toolchain-install.err
  echo $? > /work/toolchain-install.exit
  if [ "$(cat /work/toolchain-install.exit)" != 0 ]; then
    fail toolchain-install
  fi
  rustup run 1.94.0 rustc --version > /work/rustc-version.txt 2> /work/rustc-version.err || fail rustc
else
  echo 0 > /work/toolchain-install.exit
  echo "already-installed" > /work/toolchain-install.out
fi

if ! rustup run 1.94.0 rustc --version --verbose > /work/rustc-verbose.txt 2>> /work/rustc-version.err; then
  fail rustc-verbose
fi

phase() {
  name=$1
  shift
  set +e
  "$@" > "/work/${name}.out" 2> "/work/${name}.err"
  rc=$?
  set -e
  echo "$rc" > "/work/${name}.exit"
  if [ "$rc" -ne 0 ]; then
    fail "$name"
  fi
}

HELPER=/archive/testing/harness/containment-supervisor/Cargo.toml
VERIFIER=/repo/testing/harness/helper-evidence-check/Cargo.toml

sha256sum /archive/testing/harness/containment-supervisor/Cargo.lock > /work/archive-lock.sha256
sha256sum /repo/testing/harness/containment-supervisor/Cargo.lock > /work/repo-lock.sha256
sha256sum /repo/ci/deny.toml > /work/repo-deny.sha256

phase helper-fetch cargo fetch --locked --manifest-path "$HELPER"
phase helper-test cargo test --locked --offline --manifest-path "$HELPER"
phase helper-metadata cargo metadata --locked --offline --format-version 1 --manifest-path "$HELPER"
phase helper-tree cargo tree --locked --offline -e features --manifest-path "$HELPER"

phase verifier-fetch cargo fetch --locked --manifest-path "$VERIFIER"
phase verifier-test cargo test --locked --offline --manifest-path "$VERIFIER"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy-'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /work/clippy-add.out 2> /work/clippy-add.err
  add_rc=$?
  set -e
  echo "$add_rc" > /work/clippy-add.exit
  if [ "$add_rc" -ne 0 ]; then
    fail clippy-add
  fi
else
  echo 0 > /work/clippy-add.exit
  echo "already-installed" > /work/clippy-add.out
fi
phase verifier-clippy rustup run 1.94.0 cargo clippy --locked --offline --manifest-path "$VERIFIER" -- -D warnings

printf 'DONE\n' > /work/status.txt
