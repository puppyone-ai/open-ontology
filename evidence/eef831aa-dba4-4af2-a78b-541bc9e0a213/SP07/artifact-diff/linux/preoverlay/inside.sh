#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke of fixed a661dcdf oo-builder.
# The archive must not contain ontology-artifact-diff.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
echo "=== toolchain file ==="
cat rust-toolchain.toml || true

echo "=== overlay absent ==="
if [ -f crates/oo-builder/src/bin/ontology-artifact-diff.rs ] \
  || [ -f crates/oo-builder/tests/ontology_artifact_diff.rs ] \
  || [ -f contracts/builder/artifact-diff-v1.md ]
then
  echo "overlay present during preoverlay" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
if [ ! -f crates/oo-builder/src/captured_pg.rs ] || [ ! -f crates/oo-builder/src/bin/oo-builder.rs ]; then
  echo "baseline builder sources missing" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
sha256sum crates/oo-builder/src/captured_pg.rs crates/oo-builder/src/lib.rs crates/oo-builder/src/bin/oo-builder.rs \
  > /evidence/source-sha256.txt

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/lib.exit
  printf '%s\n' not_run > /evidence/captured-pg.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== cargo test oo-builder lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --lib \
  > /evidence/lib.stdout 2> /evidence/lib.stderr
lib_exit=$?
set -e
printf '%s\n' "$lib_exit" > /evidence/lib.exit
echo "PHASE lib_exit=$lib_exit"

echo "=== cargo test captured_pg ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test captured_pg \
  > /evidence/captured-pg.stdout 2> /evidence/captured-pg.stderr
captured_exit=$?
set -e
printf '%s\n' "$captured_exit" > /evidence/captured-pg.exit
echo "PHASE captured_pg_exit=$captured_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$component_exit"
fi

echo "=== cargo clippy oo-builder lib and captured_pg ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --lib --test captured_pg -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

worst=0
if [ "$lib_exit" -ne 0 ]; then worst=$lib_exit; fi
if [ "$captured_exit" -ne 0 ]; then worst=$captured_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
