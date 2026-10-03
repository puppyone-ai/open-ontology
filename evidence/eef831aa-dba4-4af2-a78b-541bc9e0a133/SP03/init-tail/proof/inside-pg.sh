#!/bin/bash
# One production TLS read. Unit phases already passed. No smoke.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_NET_OFFLINE=true
cd /src
mkdir -p /evidence/proof

echo "=== identity ==="
uname -m
rustc -Vv
cargo -V

if [ -z "${OO_A133_TLS_FIXTURE:-}" ] || [ ! -f "$OO_A133_TLS_FIXTURE" ]; then
  echo "tls fixture pointer missing" >&2
  printf '%s\n' 4 > /evidence/proof/pg-worst.exit
  exit 4
fi
if [ -f crates/oo-connectors/src/stream.rs ]; then
  echo "snapshot contains stream.rs" >&2
  printf '%s\n' 4 > /evidence/proof/pg-worst.exit
  exit 4
fi
if ! grep -q 'fn committed_init_len' crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs; then
  echo "tail correction missing" >&2
  printf '%s\n' 4 > /evidence/proof/pg-worst.exit
  exit 4
fi

echo "=== pg ==="
set +e
cargo test --manifest-path /src/Cargo.toml --locked --offline --target-dir /target \
  -p oo-connectors --test pg_wire_tls_tail -- --test-threads=1 --nocapture tls_pg_sixty_thousand_small_rows \
  > /evidence/proof/pg-test.log 2>&1
phase_exit=$?
set +e
cat /evidence/proof/pg-test.log
echo "pg_exit=$phase_exit"
printf '%s\n' "$phase_exit" > /evidence/proof/pg-test.exit
printf '%s\n' "$phase_exit" > /evidence/proof/pg-worst.exit
exit "$phase_exit"
