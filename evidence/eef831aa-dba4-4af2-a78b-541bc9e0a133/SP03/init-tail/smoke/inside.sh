#!/bin/bash
# Immutable 79515fcf archive. Lib and scoped clippy only. No source edits.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
cd /src
mkdir -p /evidence/smoke

echo "=== identity ==="
uname -m
rustc -Vv
echo "rustc_exit=$?"
cargo -V
echo "cargo_v_exit=$?"

if [ ! -f contracts/source/pgsql/empty-catalog.json ]; then
  echo "empty-catalog fixture absent" >&2
  printf '%s\n' 4 > /evidence/smoke/lib.exit
  exit 4
fi
sha256sum contracts/source/pgsql/empty-catalog.json > /evidence/smoke/fixture.sha256
echo "fixture_present=yes"

PRODUCT=crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
sha256sum \
  "$PRODUCT" \
  crates/oo-connectors/vendor/PATCH.md \
  crates/oo-connectors/tests/pg_wire_view_shrink.rs \
  > /evidence/smoke/product-source.sha256
if grep -q 'incoming view shrank' "$PRODUCT"; then
  echo "archive still has the panic expect" >&2
  printf '%s\n' 4 > /evidence/smoke/lib.exit
  exit 4
fi
if grep -q 'committed_init_len' "$PRODUCT"; then
  echo "archive already contains the tail correction" >&2
  printf '%s\n' 4 > /evidence/smoke/lib.exit
  exit 4
fi

run_phase() {
  local name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@"
  local phase_exit=$?
  set +e
  echo "${name}_exit=$phase_exit"
  printf '%s\n' "$phase_exit" > "/evidence/smoke/${name}.exit"
}

echo "=== fetch workspace ==="
run_phase fetch cargo fetch --locked --manifest-path Cargo.toml
fetch_exit=$(cat /evidence/smoke/fetch.exit)

echo "=== SMOKE lib ==="
run_phase lib cargo test --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --lib -- --test-threads=1
lib_exit=$(cat /evidence/smoke/lib.exit)

echo "=== SMOKE clippy ==="
set +e
rustup component list --toolchain 1.94.0 --installed
installed_exit=$?
set -e
echo "component_list_exit=$installed_exit"
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  run_phase clippy-add rustup component add clippy --toolchain 1.94.0
  add_exit=$(cat /evidence/smoke/clippy-add.exit)
else
  echo "clippy_add_exit=already_installed"
  add_exit=0
  printf '%s\n' 0 > /evidence/smoke/clippy-add.exit
fi
run_phase clippy cargo clippy --manifest-path Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings
clippy_exit=$(cat /evidence/smoke/clippy.exit)

worst=0
for phase_exit in "$fetch_exit" "$lib_exit" "$add_exit" "$clippy_exit"; do
  if [ "$phase_exit" -ne 0 ]; then
    worst=$phase_exit
  fi
done
echo "worst_phase_exit=$worst"
printf '%s\n' "$worst" > /evidence/smoke/worst.exit
exit "$worst"
