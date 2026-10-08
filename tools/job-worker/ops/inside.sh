#!/bin/bash
# Build and run the job-worker closed loop inside the one Rust 1.94 builder.
# TMPDIR is on the private target volume because /tmp is mounted noexec.
set -eu
set -o pipefail
cd /src/tools/job-worker
PHASE_DIR=${OO_PHASE_DIR:-/evidence/linux/run}
mkdir -p "$PHASE_DIR" /evidence/live /target/tmp
export TMPDIR=/target/tmp
chmod_evidence() { chmod -R a+rwX /evidence || true; }
trap chmod_evidence EXIT
{
  echo "HOME=$HOME"
  echo "CARGO_HOME=$CARGO_HOME"
  echo "CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-}"
  echo "CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-}"
  echo "RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN:-}"
  rustc --version
  cargo --version
} > "$PHASE_DIR/rustc.txt" 2>&1
if ! rustup component list --installed | grep -q '^clippy-'; then
  rustup component add clippy > "$PHASE_DIR/rustup-component.txt" 2>&1
fi
set +e
code=0
cargo metadata --locked --format-version 1 --no-deps > "$PHASE_DIR/metadata.txt" 2> "$PHASE_DIR/metadata.err"
meta=$?
printf '%s\n' "$meta" > "$PHASE_DIR/metadata.exit"
if [ "$meta" -ne 0 ]; then
  code=$meta
fi
if [ "$code" -eq 0 ]; then
  cargo clippy --all-targets --locked -- -D warnings > "$PHASE_DIR/clippy.txt" 2>&1
  clip=$?
  printf '%s\n' "$clip" > "$PHASE_DIR/clippy.exit"
  if [ "$clip" -ne 0 ]; then
    code=$clip
  fi
fi
if [ "$code" -eq 0 ]; then
  cargo test --locked > "$PHASE_DIR/test.txt" 2>&1
  unit=$?
  printf '%s\n' "$unit" > "$PHASE_DIR/test.exit"
  if [ "$unit" -ne 0 ]; then
    code=$unit
  fi
fi
if [ "$code" -eq 0 ]; then
  OO_JW_LIVE=1 cargo test --locked -- --ignored closed_loop > "$PHASE_DIR/closed-loop.txt" 2>&1
  live=$?
  printf '%s\n' "$live" > "$PHASE_DIR/closed-loop.exit"
  code=$live
fi
if [ -d /evidence/live ]; then
  mkdir -p "$PHASE_DIR/live"
  cp -a /evidence/live/. "$PHASE_DIR/live/" || true
fi
exit "$code"
