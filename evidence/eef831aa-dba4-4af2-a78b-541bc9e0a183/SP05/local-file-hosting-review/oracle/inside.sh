#!/bin/bash
# Independent oracle on the immutable archive plus the two review tests.
set -eu
echo "oo_task=sp05-local-file-hosting-review-oracle"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "overlay=review_oracle.rs review_memory.rs"
echo "product_source_edits=no"
uname -a
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_NET_OFFLINE=true
rustc -Vv
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
test -f crates/oo-control/tests/review_oracle.rs
test -f crates/oo-control/tests/review_memory.rs
mkdir -p /evidence/phases
cp Cargo.lock /tmp/Cargo.lock.before

if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "clippy_component=already_installed"
  printf '%s\n' 0 > /evidence/phases/clippy-component.exit
else
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  CARGO_NET_OFFLINE= rustup component add clippy --toolchain 1.94.0
  add_exit=$?
  set -e
  export CARGO_NET_OFFLINE=true
  echo "phase_exit=$add_exit phase=clippy_component"
  printf '%s\n' "$add_exit" > /evidence/phases/clippy-component.exit
  if [ "$add_exit" -ne 0 ]; then
    exit "$add_exit"
  fi
fi

echo "=== cargo tree features ==="
set +e
cargo tree --manifest-path Cargo.toml --locked --offline -p oo-control --depth 1 -e features > /evidence/features-tree.txt
tree_exit=$?
set -e
echo "phase_exit=$tree_exit phase=features_tree"
printf '%s\n' "$tree_exit" > /evidence/phases/features-tree.exit

echo "=== review_memory ==="
set +e
OO_REVIEW_OUT=/evidence/memory-notes.txt \
  cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test review_memory -- --test-threads=1 --nocapture
memory_exit=$?
set -e
echo "phase_exit=$memory_exit phase=review_memory"
printf '%s\n' "$memory_exit" > /evidence/phases/memory.exit

echo "=== review_oracle ==="
set +e
OO_REVIEW_OUT=/evidence/oracle-notes.txt \
  cargo test --manifest-path Cargo.toml --locked --offline -p oo-control --test review_oracle -- --test-threads=1 --nocapture
oracle_exit=$?
set -e
echo "phase_exit=$oracle_exit phase=review_oracle"
printf '%s\n' "$oracle_exit" > /evidence/phases/oracle.exit

echo "=== cargo clippy oo-control ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-control --all-targets -- -D warnings
clippy_exit=$?
set -e
echo "phase_exit=$clippy_exit phase=clippy"
printf '%s\n' "$clippy_exit" > /evidence/phases/clippy.exit

if ! cmp -s /tmp/Cargo.lock.before /src/Cargo.lock; then
  echo "Cargo.lock changed" >&2
  printf '%s\n' 4 > /evidence/phases/lock-after.exit
  lock_after=4
else
  printf '%s\n' 0 > /evidence/phases/lock-after.exit
  lock_after=0
fi

worst=0
for code in "$memory_exit" "$oracle_exit" "$clippy_exit" "$lock_after" "$tree_exit"; do
  if [ "$code" -gt "$worst" ]; then
    worst=$code
  fi
done
echo "phase_exit=$worst phase=worst"
printf '%s\n' "$worst" > /evidence/phases/worst.exit
exit "$worst"
