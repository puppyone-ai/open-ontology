#!/bin/bash
# Pre-overlay Linux 1.94 focused smoke of the immutable fixed archive. No overlay.
set -eu
echo "oo_task=sp05-local-hosting-download-review-pre-overlay"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "overlay=none"
echo "fixed=e3363ffecc21af4dc9fcf75509978da34b0860b1"
echo "airgap_claim=no"
echo "postgres=none"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml --locked"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test file_hosting --lib -- --nocapture"
echo "clippy_argv=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings"
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
if [ ! -f crates/oo-control/src/file_hosting.rs ] || [ ! -f crates/oo-control/tests/file_hosting.rs ]; then
  echo "archive is missing the file hosting sources" >&2
  exit 2
fi
if [ -f crates/oo-control/tests/review_download.rs ]; then
  echo "overlay is present in the pre-overlay archive" >&2
  exit 2
fi
cp Cargo.lock /tmp/Cargo.lock.before
mkdir -p /evidence/phases

ensure_component() {
  local component="$1"
  local phase="$2"
  if rustup component list --toolchain 1.94.0 --installed | grep -q "^${component}"; then
    echo "${component}_component=already_installed"
    printf '%s\n' 0 > "/evidence/phases/${phase}.exit"
    return 0
  fi
  echo "=== rustup component add ${component} --toolchain 1.94.0 ==="
  set +e
  rustup component add "$component" --toolchain 1.94.0
  local add_exit=$?
  set -e
  echo "phase_exit=${add_exit} phase=rustup_component_add_${component}"
  printf '%s\n' "$add_exit" > "/evidence/phases/${phase}.exit"
  if [ "$add_exit" -ne 0 ]; then
    exit "$add_exit"
  fi
}

ensure_component rustfmt rustfmt-component
ensure_component clippy clippy-component

echo "=== cargo fmt --check -p oo-control ==="
set +e
cargo fmt --check -p oo-control
fmt_exit=$?
set -e
echo "phase_exit=$fmt_exit phase=cargo_fmt_check"
printf '%s\n' "$fmt_exit" > /evidence/phases/fmt-check.exit
if [ "$fmt_exit" -ne 0 ]; then
  echo "fmt check failed; archive left unchanged" >&2
  exit "$fmt_exit"
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked
fetch_exit=$?
set -e
echo "phase_exit=$fetch_exit phase=cargo_fetch"
printf '%s\n' "$fetch_exit" > /evidence/phases/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi
if ! cmp -s /tmp/Cargo.lock.before /src/Cargo.lock; then
  echo "Cargo.lock changed during fetch" >&2
  printf '%s\n' 4 > /evidence/phases/lock-unchanged.exit
  exit 4
fi
echo "phase_exit=0 phase=lock_unchanged"
printf '%s\n' 0 > /evidence/phases/lock-unchanged.exit

echo "=== cargo test file_hosting ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test file_hosting --lib -- --nocapture
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test_file_hosting"
printf '%s\n' "$test_exit" > /evidence/phases/test.exit
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== cargo clippy oo-control ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=cargo_clippy"
printf '%s\n' "$clippy_exit" > /evidence/phases/clippy.exit
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

if ! cmp -s /tmp/Cargo.lock.before /src/Cargo.lock; then
  echo "Cargo.lock changed during the smoke" >&2
  printf '%s\n' 4 > /evidence/phases/lock-after.exit
  exit 4
fi
echo "phase_exit=0 phase=lock_after"
printf '%s\n' 0 > /evidence/phases/lock-after.exit
echo "phase_exit=0 phase=worst"
printf '%s\n' 0 > /evidence/phases/worst.exit
echo "pre_overlay_smoke=pass"
