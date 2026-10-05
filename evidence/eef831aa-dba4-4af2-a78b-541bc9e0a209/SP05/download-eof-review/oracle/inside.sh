#!/bin/bash
# Independent overlay on the immutable fixed archive. No PostgreSQL. No dependency edits.
set -euo pipefail
echo "oo_task=sp05-download-eof-review-oracle"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
echo "postgres=none"
echo "overlay=review_download.rs"
echo "fetch_argv=cargo fetch --manifest-path Cargo.toml --locked"
echo "fmt_argv=cargo fmt -p oo-control -- --check"
echo "test_argv=cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test review_download -- --nocapture --test-threads=1"
echo "clippy_argv=cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export OO_A209_NOTES=/evidence/notes
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
overlay=crates/oo-control/tests/review_download.rs
if [ ! -f "$overlay" ]; then
  echo "review overlay is missing from the archive" >&2
  exit 2
fi
for path in \
  crates/oo-control/src/file_hosting.rs \
  crates/oo-control/tests/file_hosting.rs \
  contracts/control/local-file-hosting-v1.md
do
  if [ ! -f "$path" ]; then
    echo "missing $path" >&2
    exit 2
  fi
done

product_files() {
  printf '%s\n' \
    crates/oo-control/src/file_hosting.rs \
    crates/oo-control/tests/file_hosting.rs \
    crates/oo-control/src/lib.rs \
    crates/oo-control/Cargo.toml \
    contracts/control/local-file-hosting-v1.md \
    Cargo.lock
}

mkdir -p /evidence/phases /evidence/notes /evidence/ran
product_files | while read -r path; do
  sha256sum "$path"
done > /tmp/product.before
cp Cargo.lock /tmp/Cargo.lock.before
sha256sum "$overlay" > /tmp/overlay.before

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

echo "=== rustfmt review_download.rs ==="
set +e
rustfmt --edition 2024 --check "$overlay"
fmt_one=$?
set -e
if [ "$fmt_one" -ne 0 ]; then
  echo "overlay_rustfmt=rewritten"
  rustfmt --edition 2024 "$overlay"
else
  echo "overlay_rustfmt=already_clean"
fi
cp "$overlay" /evidence/ran/review_download.rs
sha256sum "$overlay" > /evidence/ran/overlay.sha256

echo "=== cargo fmt --check oo-control ==="
set +e
cargo fmt -p oo-control -- --check
fmt_check_exit=$?
set -e
echo "phase_exit=$fmt_check_exit phase=cargo_fmt_check"
printf '%s\n' "$fmt_check_exit" > /evidence/phases/fmt-check.exit
if [ "$fmt_check_exit" -ne 0 ]; then
  exit "$fmt_check_exit"
fi

product_files | while read -r path; do
  sha256sum "$path"
done > /tmp/product.after
if ! cmp -s /tmp/product.before /tmp/product.after; then
  echo "product bytes changed under the overlay" >&2
  diff -u /tmp/product.before /tmp/product.after || true
  printf '%s\n' 5 > /evidence/phases/product-unchanged.exit
  exit 5
fi
echo "phase_exit=0 phase=product_unchanged"
printf '%s\n' 0 > /evidence/phases/product-unchanged.exit
cp /tmp/product.after /evidence/ran/product.sha256

echo "=== cargo test review_download ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test review_download -- --nocapture --test-threads=1
test_exit=$?
set -e
echo "phase_exit=$test_exit phase=cargo_test_review_download"
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
  echo "Cargo.lock changed during the overlay run" >&2
  printf '%s\n' 4 > /evidence/phases/lock-after.exit
  exit 4
fi
echo "phase_exit=0 phase=lock_after"
printf '%s\n' 0 > /evidence/phases/lock-after.exit
echo "phase_exit=0 phase=worst"
printf '%s\n' 0 > /evidence/phases/worst.exit
echo "oracle=pass"
