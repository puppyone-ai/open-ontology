#!/bin/bash
# Focused Linux/arm64 Rust 1.94 smoke of the fixed archive, then the author
# pg_wire_bounds test, then the independent review probe.
# airgap_claim=no. Fetch uses the network. Later phases pass --offline.
set -u
echo "oo_task=wire-bounds-review"
echo "oo_source=a90c86b405febebfdc70f8169be42dc4de4c3dfc"
echo "oo_tree=6e7eb46c09470a8163a324063beef48c6254bbb1"
echo "oo_parent=6de125ab01df6564024005dcc74fbf9d0a96b392"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
echo "airgap_claim=no"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export GIT_CONFIG_GLOBAL=/tmp/oo-a122-gitconfig
export GIT_CONFIG_SYSTEM=/tmp/oo-a122-gitsystem
: > "$GIT_CONFIG_GLOBAL"
: > "$GIT_CONFIG_SYSTEM"
git config --file "$GIT_CONFIG_GLOBAL" user.name a122-review
git config --file "$GIT_CONFIG_GLOBAL" user.email a122-review@invalid.example
git config --file "$GIT_CONFIG_GLOBAL" init.defaultBranch ephemeral
rustc -Vv
cargo -V
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
host_line=$(rustc -Vv | awk '/^host:/{print $2}')
printf 'rustc_host=%s\n' "$host_line"
case "$host_line" in
  aarch64-unknown-linux-gnu) ;;
  *) echo "refusing host: $host_line" >&2; printf '%s\n' 2 > /evidence/toolchain.exit; exit 2 ;;
esac
printf '%s\n' 0 > /evidence/toolchain.exit

cd /src
if [ ! -f contracts/source/pgsql/empty-catalog.json ]; then
  echo "snapshot missing catalog" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ ! -f crates/oo-connectors/tests/pg_wire_bounds.rs ]; then
  echo "snapshot missing author pg_wire_bounds" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if [ -f crates/oo-connectors/tests/pg_wire_bounds_review.rs ]; then
  echo "snapshot already contains the review probe" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
if ! grep -q PgIncomingBounds crates/oo-connectors/vendor/sqlx-postgres/src/connection/stream.rs; then
  echo "fixed archive missing PgIncomingBounds" >&2
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
echo "=== sha256sum -c product-blobs.sha256 ==="
set +e
sha256sum -c /evidence/product-blobs.sha256 > /evidence/product-blobs.check 2>&1
hash_exit=$?
set +e
echo "phase_exit=$hash_exit phase=product_blobs"
printf '%s\n' "$hash_exit" > /evidence/product-blobs.exit
if [ "$hash_exit" -ne 0 ]; then
  printf '%s\n' 2 > /evidence/snapshot.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/snapshot.exit

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  echo "=== rustup component add clippy --toolchain 1.94.0 ==="
  set +e
  rustup component add clippy --toolchain 1.94.0
  clippy_add=$?
  set +e
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
set +e
echo "phase_exit=$fetch_exit phase=cargo_fetch_locked"
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
if [ "$fetch_exit" -ne 0 ]; then
  echo "elapsed_seconds=$SECONDS"
  exit "$fetch_exit"
fi

echo "=== cargo test --lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --lib
lib_exit=$?
set +e
echo "phase_exit=$lib_exit phase=cargo_test_oo_connectors_lib"
printf '%s\n' "$lib_exit" > /evidence/lib.exit

echo "=== cargo clippy -p oo-connectors --all-targets -- -D warnings ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings
clippy_exit=$?
set +e
echo "phase_exit=$clippy_exit phase=cargo_clippy_oo_connectors"
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit

echo "=== cargo test --test pg_wire_bounds ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_bounds -- --nocapture --test-threads=1
bounds_exit=$?
set +e
echo "phase_exit=$bounds_exit phase=cargo_test_pg_wire_bounds"
printf '%s\n' "$bounds_exit" > /evidence/bounds.exit

echo "=== overlay review probe ==="
cp /probe/pg_wire_bounds_review.rs crates/oo-connectors/tests/pg_wire_bounds_review.rs
set +e
sha256sum -c /evidence/product-blobs.sha256 > /evidence/product-blobs-after-overlay.check 2>&1
after_exit=$?
set +e
echo "phase_exit=$after_exit phase=product_blobs_after_overlay"
printf '%s\n' "$after_exit" > /evidence/product-blobs-after.exit
if [ "$after_exit" -ne 0 ]; then
  echo "overlay changed a fixed product blob" >&2
  echo "elapsed_seconds=$SECONDS"
  exit "$after_exit"
fi
sha256sum crates/oo-connectors/tests/pg_wire_bounds_review.rs > /evidence/probe.sha256

echo "=== cargo test --test pg_wire_bounds_review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_bounds_review -- --nocapture --test-threads=1
review_exit=$?
set +e
echo "phase_exit=$review_exit phase=cargo_test_pg_wire_bounds_review"
printf '%s\n' "$review_exit" > /evidence/review.exit

echo "=== cargo clippy review test ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --test pg_wire_bounds_review -- -D warnings
review_clippy_exit=$?
set +e
echo "phase_exit=$review_clippy_exit phase=cargo_clippy_pg_wire_bounds_review"
printf '%s\n' "$review_clippy_exit" > /evidence/clippy-review.exit

echo "elapsed_seconds=$SECONDS"
if [ "$lib_exit" -ne 0 ]; then
  exit "$lib_exit"
fi
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi
if [ "$bounds_exit" -ne 0 ]; then
  exit "$bounds_exit"
fi
if [ "$review_exit" -ne 0 ]; then
  exit "$review_exit"
fi
exit "$review_clippy_exit"
