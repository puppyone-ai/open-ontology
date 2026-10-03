#!/bin/bash
# Patched archive. Lib, bounds, accounting, portal, plaintext view, init_mut tail, scoped clippy.
# PostgreSQL runs only when OO_A133_TLS_FIXTURE is set.
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

if [ ! -f contracts/source/pgsql/empty-catalog.json ]; then
  echo "empty-catalog fixture absent" >&2
  printf '%s\n' 4 > /evidence/proof/worst.exit
  exit 4
fi
sha256sum contracts/source/pgsql/empty-catalog.json > /evidence/proof/fixture.sha256

PRODUCT=crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
sha256sum \
  "$PRODUCT" \
  crates/oo-connectors/vendor/PATCH.md \
  crates/oo-connectors/tests/pg_wire_view_shrink.rs \
  crates/oo-connectors/tests/pg_wire_tls_tail.rs \
  > /evidence/proof/product-source.sha256
if ! grep -q 'fn committed_init_len' "$PRODUCT"; then
  echo "tail correction missing" >&2
  printf '%s\n' 4 > /evidence/proof/worst.exit
  exit 4
fi
if grep -q 'incoming view shrank' "$PRODUCT"; then
  echo "panic expect returned" >&2
  printf '%s\n' 4 > /evidence/proof/worst.exit
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
  printf '%s\n' "$phase_exit" > "/evidence/proof/${name}.exit"
  if [ "$phase_exit" -ne 0 ]; then
    printf '%s\n' "$phase_exit" > /evidence/proof/worst.exit
    echo "worst_phase=$name"
    exit "$phase_exit"
  fi
}

run_phase fetch cargo fetch --locked --manifest-path Cargo.toml

export OO_A133_PRODUCT=/src/crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
export OO_A133_REQUIRE_PRODUCT=1
CARGO_OPTS=(--manifest-path Cargo.toml --locked --offline --target-dir /target)

run_phase lib cargo test "${CARGO_OPTS[@]}" -p oo-connectors --lib -- --test-threads=1
run_phase bounds cargo test "${CARGO_OPTS[@]}" -p oo-connectors --test pg_wire_bounds -- --test-threads=1
run_phase accounting cargo test "${CARGO_OPTS[@]}" -p oo-connectors --test pg_wire_accounting -- --test-threads=1
run_phase portal cargo test "${CARGO_OPTS[@]}" -p oo-connectors --test pg_portal_bounds -- --test-threads=1
run_phase view cargo test "${CARGO_OPTS[@]}" -p oo-connectors --test pg_wire_view_shrink -- --test-threads=1
run_phase tail cargo test "${CARGO_OPTS[@]}" -p oo-connectors --test pg_wire_tls_tail -- --test-threads=1 --skip tls_pg
# sqlx-core is a patched path dependency, not a workspace member. Cargo
# rejects --features for that package. The workspace graph already enables
# the production features, and bytes stays 1.12.1 via the workspace lock.
run_phase sqlx-core cargo test "${CARGO_OPTS[@]}" -p sqlx-core --lib -- --test-threads=1 committed_init_len init_mut_small_frames
run_phase clippy cargo clippy "${CARGO_OPTS[@]}" -p oo-connectors --all-targets -- -D warnings

if [ -n "${OO_A133_TLS_FIXTURE:-}" ]; then
  run_phase pg cargo test "${CARGO_OPTS[@]}" -p oo-connectors --test pg_wire_tls_tail -- --test-threads=1 tls_pg_sixty_thousand_small_rows
else
  echo "pg_phase=not_requested"
  printf '%s\n' skip > /evidence/proof/pg.exit
fi

printf '%s\n' 0 > /evidence/proof/worst.exit
echo "worst_phase_exit=0"
