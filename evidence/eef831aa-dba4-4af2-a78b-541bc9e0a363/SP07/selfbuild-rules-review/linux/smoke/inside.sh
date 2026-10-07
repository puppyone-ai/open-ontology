#!/bin/bash
# Toolchain baseline before the review proof. No product mount. No cargo update.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a354/SP07/selfbuild-rules/linux/smoke/inside.sh
# Change: owner a363. Clippy absence is the image baseline and ends the smoke.
set -u
echo "oo_task=sp07-selfbuild-rules-review-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a363"
echo "oo_fixed=e5427c1a18e2ddbb480791d214c76888eb7ca998"
echo "oo_platform=linux/arm64"
echo "phase=smoke"
uname -a > /evidence/linux/smoke/uname.txt
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
mkdir -p /evidence/linux/smoke/phases /cargo-home

record() {
  printf '%s\n' "$2" > "/evidence/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/smoke/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/smoke/rustc-version.txt
rustc -Vv > /evidence/linux/smoke/rustc-verbose.txt
cargo -V | tee /evidence/linux/smoke/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) record toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

if rustup component list --installed | grep -q '^clippy-'; then
  printf '%s\n' present > /evidence/linux/smoke/clippy-component.txt
  record clippy-component 0
else
  printf '%s\n' absent > /evidence/linux/smoke/clippy-component.txt
  record clippy-component 1
fi

if [ -e /src/tools/selfbuild-rules ]; then
  printf '%s\n' present > /evidence/linux/smoke/product-mount.txt
  record product-mount 2
fi
printf '%s\n' absent > /evidence/linux/smoke/product-mount.txt
record product-mount 0

printf '%s\n' no > /evidence/linux/smoke/cargo-update-invoked.txt
printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo smoke_done
