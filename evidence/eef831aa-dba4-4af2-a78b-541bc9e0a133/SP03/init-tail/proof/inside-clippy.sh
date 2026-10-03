#!/bin/bash
# Scoped clippy only. The isolated sqlx-core tests already passed on this
# snapshot. Smoke is not repeated. Clippy is not in the image layer, so this
# installs the 1.94.0 component the same way the green smoke did.
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
rustup show

PRODUCT=crates/oo-connectors/vendor/sqlx-core/src/net/socket/buffered.rs
if ! grep -q 'fn committed_init_len' "$PRODUCT"; then
  echo "tail correction missing from snapshot" >&2
  printf '%s\n' 4 > /evidence/proof/clippy-worst.exit
  exit 4
fi
if [ -f crates/oo-connectors/src/stream.rs ]; then
  echo "snapshot contains stream.rs" >&2
  printf '%s\n' 4 > /evidence/proof/clippy-worst.exit
  exit 4
fi

run_phase() {
  local name="$1"
  shift
  echo "=== $name ==="
  set +e
  "$@" > "/evidence/proof/clippy-${name}.log" 2>&1
  local phase_exit=$?
  set +e
  cat "/evidence/proof/clippy-${name}.log"
  echo "${name}_exit=$phase_exit"
  printf '%s\n' "$phase_exit" > "/evidence/proof/clippy-${name}.exit"
  if [ "$phase_exit" -ne 0 ]; then
    printf '%s\n' "$phase_exit" > /evidence/proof/clippy-worst.exit
    echo "worst_phase=$name"
    exit "$phase_exit"
  fi
}

echo "=== components ==="
rustup component list --toolchain 1.94.0 --installed > /evidence/proof/clippy-components-before.txt
cat /evidence/proof/clippy-components-before.txt
if ! grep -q '^clippy' /evidence/proof/clippy-components-before.txt; then
  run_phase clippy-add rustup component add clippy --toolchain 1.94.0
else
  echo "clippy_add_exit=already_installed"
  printf '%s\n' 0 > /evidence/proof/clippy-clippy-add.exit
fi

run_phase clippy cargo clippy --manifest-path /src/Cargo.toml --locked --offline --target-dir /target -p oo-connectors --all-targets -- -D warnings

printf '%s\n' 0 > /evidence/proof/clippy-worst.exit
echo "clippy_worst_exit=0"
