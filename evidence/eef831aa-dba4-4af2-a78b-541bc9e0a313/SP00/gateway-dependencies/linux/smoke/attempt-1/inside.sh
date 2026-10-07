#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of frozen oo-arch-check.
# Archive is commit b91b67bee0f603725b2cb7cc9c284588a2699392. No manifest edit.
# FAILED ATTEMPT. cargo 1.94.0 fetch does not accept -p. Preserved, not re-run.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
cd /src

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
rustup show | tee /evidence/rustup-show.txt
printf '%s\n' "toolchain_file_channel=$(sed -n 's/^channel = \"\\(.*\\)\"/\\1/p' rust-toolchain.toml)" | tee -a /evidence/rustc-version.txt

if ! grep -q 'release: 1.94' /evidence/rustc-version.txt; then
  echo "compiler is not rustc 1.94" >&2
  printf '%s\n' 2 > /evidence/check.exit
  exit 2
fi

echo "=== frozen manifests ==="
sha256sum Cargo.toml Cargo.lock > /evidence/manifest-sha256.txt
if [ -f /probe/Cargo.toml ]; then
  echo "probe manifest is visible inside the frozen archive mount" >&2
  printf '%s\n' 2 > /evidence/probe-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/probe-absent.exit

echo "=== cargo fetch locked oo-arch-check ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked -p oo-arch-check > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/metadata.exit
  printf '%s\n' not_run > /evidence/check.exit
  exit "$fetch_exit"
fi

echo "=== cargo metadata locked ==="
set +e
cargo metadata --manifest-path Cargo.toml --locked --format-version 1 --no-deps > /evidence/metadata.stdout 2> /evidence/metadata.stderr
metadata_exit=$?
set -e
printf '%s\n' "$metadata_exit" > /evidence/metadata.exit
echo "PHASE metadata_exit=$metadata_exit"
if [ "$metadata_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/check.exit
  exit "$metadata_exit"
fi

echo "=== cargo check oo-arch-check offline ==="
set +e
cargo check --manifest-path Cargo.toml --locked --offline -p oo-arch-check > /evidence/check.stdout 2> /evidence/check.stderr
check_exit=$?
set -e
printf '%s\n' "$check_exit" > /evidence/check.exit
echo "PHASE check_exit=$check_exit"
sha256sum Cargo.toml Cargo.lock > /evidence/manifest-sha256-after.txt
if ! cmp -s /evidence/manifest-sha256.txt /evidence/manifest-sha256-after.txt; then
  echo "frozen manifest hash changed during smoke" >&2
  printf '%s\n' 2 > /evidence/manifest-unchanged.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/manifest-unchanged.exit
echo "inside_ok=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
exit "$check_exit"
