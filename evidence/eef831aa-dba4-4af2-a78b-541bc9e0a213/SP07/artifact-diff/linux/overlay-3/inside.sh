#!/bin/bash
# Focused Linux arm64 Rust 1.94 test and Clippy of ontology-artifact-diff.
# The archive is fixed a661dcdf plus the two owned Rust files.
# Clippy is split: combined --bin --test did not define CARGO_BIN_EXE.
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

echo "=== overlay present ==="
if [ ! -f crates/oo-builder/src/bin/ontology-artifact-diff.rs ] \
  || [ ! -f crates/oo-builder/tests/ontology_artifact_diff.rs ]
then
  echo "overlay files missing" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
if [ ! -f crates/oo-builder/src/captured_pg.rs ] || [ ! -f crates/oo-builder/src/bin/oo-builder.rs ]; then
  echo "baseline builder sources missing" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit
sha256sum \
  crates/oo-builder/src/bin/ontology-artifact-diff.rs \
  crates/oo-builder/tests/ontology_artifact_diff.rs \
  crates/oo-builder/src/captured_pg.rs \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/bin/oo-builder.rs \
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
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== cargo test ontology_artifact_diff ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test ontology_artifact_diff \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

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

echo "=== cargo clippy bin ontology-artifact-diff ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --bin ontology-artifact-diff -- -D warnings \
  > /evidence/clippy-bin.stdout 2> /evidence/clippy-bin.stderr
clippy_bin_exit=$?
set -e
printf '%s\n' "$clippy_bin_exit" > /evidence/clippy-bin.exit
echo "PHASE clippy_bin_exit=$clippy_bin_exit"

echo "=== cargo clippy test ontology_artifact_diff ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --test ontology_artifact_diff -- -D warnings \
  > /evidence/clippy-test.stdout 2> /evidence/clippy-test.stderr
clippy_test_exit=$?
set -e
printf '%s\n' "$clippy_test_exit" > /evidence/clippy-test.exit
echo "PHASE clippy_test_exit=$clippy_test_exit"

{
  echo "=== bin ==="
  cat /evidence/clippy-bin.stdout
  echo "=== test ==="
  cat /evidence/clippy-test.stdout
} > /evidence/clippy.stdout
{
  echo "=== bin ==="
  cat /evidence/clippy-bin.stderr
  echo "=== test ==="
  cat /evidence/clippy-test.stderr
} > /evidence/clippy.stderr
clippy_exit=0
if [ "$clippy_bin_exit" -ne 0 ]; then clippy_exit=$clippy_bin_exit; fi
if [ "$clippy_test_exit" -ne 0 ]; then clippy_exit=$clippy_test_exit; fi
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
