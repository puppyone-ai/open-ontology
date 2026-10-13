#!/bin/bash
# Install Clippy 1.94 into the private rustup volume. No workspace fetch or test.
set -eu
export HOME=/tmp
export RUSTUP_TOOLCHAIN=1.94.0
export TMPDIR=/tmp
mkdir -p /tmp /evidence/phases
rustc --version | tee /evidence/rustc-version.txt
uname -m | tee /evidence/uname.txt
case "$(cat /evidence/rustc-version.txt)" in
  "rustc 1.94.0 "*) printf '%s\n' 0 > /evidence/phases/toolchain.exit ;;
  *) echo "refusing toolchain" >&2; printf '%s\n' 2 > /evidence/phases/toolchain.exit; exit 2 ;;
esac
grep -q aarch64 /evidence/uname.txt
printf '%s\n' 0 > /evidence/phases/arch.exit
toolchain=1.94.0-aarch64-unknown-linux-gnu
if [ ! -x "/rustup/toolchains/${toolchain}/bin/cargo-clippy" ]; then
  if [ ! -d "/rustup/toolchains/${toolchain}" ]; then
    cp -a /usr/local/rustup/. /rustup/
  fi
  export RUSTUP_HOME=/rustup
  rustup component add clippy --toolchain 1.94.0
fi
export RUSTUP_HOME=/rustup
rustup component list --toolchain 1.94.0 --installed | tee /evidence/components.txt
cargo clippy --version | tee /evidence/clippy-version.txt
grep -q 'clippy 0.1.94' /evidence/clippy-version.txt
printf '%s\n' 0 > /evidence/phases/install.exit
