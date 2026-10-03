#!/bin/bash
# Continuation after the third proof. Lib, bounds, accounting, portal, view, and
# the init_mut tail tests already passed on this snapshot. This run records the
# tail peaks, the embedded sqlx-core tests, and scoped clippy. No smoke.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
cd /src
mkdir -p /evidence/proof

echo "=== identity ==="
uname -m
rustc -Vv
cargo -V

PRODUCT=crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
if ! grep -q 'fn committed_init_len' "$PRODUCT"; then
  echo "tail correction missing from snapshot" >&2
  printf '%s\n' 4 > /evidence/proof/rest-worst.exit
  exit 4
fi
sha256sum "$PRODUCT" crates/oo-connectors/vendor/PATCH.md crates/oo-connectors/tests/pg_wire_tls_tail.rs > /evidence/proof/rest-product-source.sha256

run_phase() {
  local name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@"
  local phase_exit=$?
  set +e
  echo "${name}_exit=$phase_exit"
  printf '%s\n' "$phase_exit" > "/evidence/proof/rest-${name}.exit"
  if [ "$phase_exit" -ne 0 ]; then
    printf '%s\n' "$phase_exit" > /evidence/proof/rest-worst.exit
    echo "worst_phase=$name"
    exit "$phase_exit"
  fi
}

export OO_A133_PRODUCT=/src/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
export OO_A133_REQUIRE_PRODUCT=1
OPTS=(--manifest-path Cargo.toml --locked --offline --target-dir /target)

run_phase tail cargo test "${OPTS[@]}" -p oo-connectors --test pg_wire_tls_tail -- --test-threads=1 --skip tls_pg --nocapture
run_phase sqlx-core cargo test "${OPTS[@]}" -p sqlx-core --lib -- --test-threads=1 --nocapture committed_init_len init_mut_small_frames
run_phase clippy cargo clippy "${OPTS[@]}" -p oo-connectors --all-targets -- -D warnings

printf '%s\n' 0 > /evidence/proof/rest-worst.exit
echo "rest_worst_exit=0"
