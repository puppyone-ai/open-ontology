#!/bin/bash
# Post-edit tests and Clippy of the overlaid query-result-export crate.
# Linux arm64 Rust 1.94. No cargo fetch. No cargo update. No PostgreSQL.
set -u
echo "oo_task=sp05-query-result-export-decimal-postedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a333"
echo "oo_fixed=63566b60aabdf29a29bfe55c4866edf3c4081cfd"
echo "oo_platform=linux/arm64"
echo "phase=postedit"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_NET_OFFLINE=true
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/postedit/phases /cargo-home/bin
printf '%s\n' "not-invoked" > /evidence/linux/postedit/cargo-update-invoked.txt

record() {
  mkdir -p /evidence/linux/postedit/phases
  printf '%s\n' "$2" > "/evidence/linux/postedit/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/postedit/postedit.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/postedit/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
record toolchain 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  record sha256-tool $?
else
  printf '%s\n' 0 > /evidence/linux/postedit/phases/sha256-tool.exit
fi
/cargo-home/bin/sha256 file /evidence/freeze/AMENDMENT.md > /evidence/linux/postedit/amendment-sha256.txt
record amendment-hash $?
if ! grep -q '4154e28f750f2f36d62b4684e4382e0e19fd1e46b1361c504a1f0b49fd86ecd2' /evidence/linux/postedit/amendment-sha256.txt; then
  echo "amendment hash changed after the freeze" >&2
  record amendment-stable 1
fi
record amendment-stable 0
/cargo-home/bin/sha256 file $(find /src -type f | sort) > /evidence/linux/postedit/overlay-sha256.txt
record overlay-hash $?

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/postedit/clippy-component.log 2>&1
  record clippy-component $?
else
  echo "clippy_component=already_installed" > /evidence/linux/postedit/clippy-component.log
  record clippy-component 0
fi

cd /src
cargo test --locked --offline -- --test-threads=1 > /evidence/linux/postedit/test.log 2>&1
record test $?
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/linux/postedit/clippy.log 2>&1
record clippy $?
printf '%s\n' 0 > /evidence/linux/postedit/postedit.exit
echo "postedit_done"
