#!/bin/bash
# Focused locked check of the archived job-worker tree. Does not run the ignored live closed_loop.
set -eu
set -o pipefail
cd /src/tools/job-worker
PHASE=${OO_PHASE_DIR:-/evidence/linux/run}
mkdir -p "$PHASE" /target/tmp
export TMPDIR=/target/tmp
chmod_evidence() { chmod -R a+rwX /evidence || true; }
trap chmod_evidence EXIT
{
  echo "HOME=$HOME"
  echo "CARGO_HOME=$CARGO_HOME"
  echo "CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-}"
  echo "CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-}"
  echo "RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN:-}"
  rustc --version --verbose
  cargo --version
} > "$PHASE/rustc.txt" 2>&1
if ! rustup component list --installed | grep -q '^clippy-'; then
  rustup component add clippy > "$PHASE/rustup-component.txt" 2>&1
fi
code=0
set +e
cargo metadata --locked --format-version 1 --no-deps > "$PHASE/metadata.txt" 2> "$PHASE/metadata.err"
meta=$?
printf '%s\n' "$meta" > "$PHASE/metadata.exit"
if [ "$meta" -ne 0 ]; then
  code=$meta
fi
if [ "$code" -eq 0 ]; then
  cargo clippy --all-targets --locked -- -D warnings > "$PHASE/clippy.txt" 2>&1
  clip=$?
  printf '%s\n' "$clip" > "$PHASE/clippy.exit"
  if [ "$clip" -ne 0 ]; then
    code=$clip
  fi
fi
if [ "$code" -eq 0 ]; then
  cargo test --locked > "$PHASE/test.txt" 2>&1
  unit=$?
  printf '%s\n' "$unit" > "$PHASE/test.exit"
  if [ "$unit" -ne 0 ]; then
    code=$unit
  fi
fi
if [ "$code" -eq 0 ]; then
  cargo build --locked --bin job-worker > "$PHASE/build.txt" 2>&1
  built=$?
  printf '%s\n' "$built" > "$PHASE/build.exit"
  if [ "$built" -ne 0 ]; then
    code=$built
  fi
fi
if [ "$code" -eq 0 ]; then
  /target/debug/job-worker --config /secrets/jw.conf reset > "$PHASE/reset.txt" 2>&1
  reset_code=$?
  printf '%s\n' "$reset_code" > "$PHASE/reset.exit"
  if [ "$reset_code" -ne 0 ]; then
    code=$reset_code
  fi
fi
if [ "$code" -eq 0 ]; then
  /target/debug/job-worker --config /secrets/jw.conf apply > "$PHASE/apply.txt" 2>&1
  apply_code=$?
  printf '%s\n' "$apply_code" > "$PHASE/apply.exit"
  if [ "$apply_code" -ne 0 ]; then
    code=$apply_code
  fi
fi
printf '%s\n' "$code" > "$PHASE/build-phase.exit"
exit "$code"
