#!/bin/bash
# Focused smoke of archive fb2d1d19. One package, linux/arm64, Rust 1.94.
# Not a dual-arch matrix, not an air-gap claim, not a supply-chain pass.
set -u
echo "oo_task=isc-attribution-review-smoke"
echo "oo_source=fb2d1d19c21b510dfd6231c3b67d0efbf4285491"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "package=oo-core"
echo "package_reason=commit diff has no Rust sources; oo-core is the foundation crate in this archive"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml --locked"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-core"
echo "clippy_argv=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-core --all-targets -- -D warnings"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a078-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a078-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a078-review
git config --file "$GIT_CONFIG_GLOBAL" user.email a078-review@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
echo "RUSTUP_TOOLCHAIN=$RUSTUP_TOOLCHAIN"
echo "CARGO_HOME=$CARGO_HOME"
echo "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
echo "HOME=$HOME"
echo "GIT_CONFIG_GLOBAL=$GIT_CONFIG_GLOBAL"
rustc -Vv
cargo -V
rustup show
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
printf '%s\n' 0 > /evidence/toolchain.exit
cd /src
if [ ! -f Cargo.lock ] || [ ! -f Cargo.toml ] || [ ! -f crates/oo-core/Cargo.toml ]; then
  echo "snapshot missing cargo inputs" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/snapshot.exit
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set -e
  echo "phase_exit=$clippy_add phase=rustup_component_add_clippy"
  printf '%s\n' "$clippy_add" > /evidence/clippy-component.exit
  if [ "$clippy_add" -ne 0 ]; then
    exit "$clippy_add"
  fi
else
  echo "clippy_component=already_installed"
  printf '%s\n' 0 > /evidence/clippy-component.exit
fi
echo "=== cargo fetch --manifest-path Cargo.toml --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch_locked"
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
echo "=== cargo test --manifest-path Cargo.toml --locked --offline -p oo-core ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-core
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test_oo_core"
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "=== cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-core --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-core --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy_oo_core"
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "elapsed_seconds=$SECONDS"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi
exit "$clippy_exit"
