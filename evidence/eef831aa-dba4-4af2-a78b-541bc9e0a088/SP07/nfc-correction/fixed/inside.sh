#!/bin/bash
# Focused oo-builder verification of the NFC correction snapshot.
# Fetch has no -p. Offline test and clippy follow only if fetch exits 0.
set -u
echo "oo_task=sp07-w01-b-nfc-correction-fixed"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml --locked"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder"
echo "clippy_argv=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings"
if [ -f /src/SNAPSHOT.txt ]; then
  cat /src/SNAPSHOT.txt
fi
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
echo "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f Cargo.lock ] || [ ! -f crates/oo-builder/src/nfc.rs ] || [ -f crates/oo-builder/src/nfc_tables.rs ]; then
  echo "snapshot is not the corrected builder" >&2
  exit 2
fi
mkdir -p /target/phases
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set -e
  echo "phase_exit=$clippy_add phase=rustup_component_add_clippy"
  printf '%s\n' "$clippy_add" > /target/phases/clippy-component.exit
  if [ "$clippy_add" -ne 0 ]; then
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  printf '%s\n' 0 > /target/phases/clippy-component.exit
fi
echo "=== cargo fetch --manifest-path Cargo.toml --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch_locked"
printf '%s\n' "$fetch_exit" > /target/phases/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
echo "=== cargo tree -p oo-builder -i unicode-normalization --locked --offline ==="
set +e
cargo tree -p oo-builder -i unicode-normalization --locked --offline
tree_exit=$?
set -e
echo "phase_exit=$tree_exit phase=cargo_tree_unicode_normalization"
printf '%s\n' "$tree_exit" > /target/phases/tree.exit
echo "=== cargo tree -p oo-builder -e features -i unicode-normalization --locked --offline ==="
cargo tree -p oo-builder -e features -i unicode-normalization --locked --offline || true
echo "=== cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test_oo_builder"
printf '%s\n' "$test_exit" > /target/phases/test.exit
echo "=== cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy_oo_builder"
printf '%s\n' "$clippy_exit" > /target/phases/clippy.exit
echo "elapsed_seconds=$SECONDS"
worst=0
for phase_exit in "$test_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /target/phases/worst.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
exit "$clippy_exit"
