#!/bin/bash
# Linux Rust 1.94. Probe package only. Does not compile pipeline, pg-builder, or source-semantics.
set -euo pipefail

cd /src
rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
grep -q '^rustc 1.94.0 ' /evidence/rustc-version.txt
if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.txt 2>&1
fi

if ! cargo metadata --locked --offline --format-version 1 >/evidence/probe-metadata.txt 2>/evidence/probe-metadata.err; then
  cargo fetch --locked > /evidence/probe-fetch.txt 2>&1
  printf 'fetched\n' > /evidence/probe-fetch.result
else
  printf 'cached\n' > /evidence/probe-fetch.result
fi

run_step() {
  local name="$1"
  shift
  set +e
  "$@" >"/evidence/${name}.txt" 2>&1
  local rc=$?
  set -e
  printf '%s\n' "$rc" >"/evidence/${name}.exit"
  if [[ "$rc" -ne 0 ]]; then
    printf 'FAILED %s %s\n' "$name" "$rc" > /evidence/status.txt
    exit "$rc"
  fi
}

run_step probe-test cargo test --locked --offline --jobs 1 -p oo-mongodb-driver-probe -- --test-threads=1
run_step probe-clippy cargo clippy --locked --offline --jobs 1 -p oo-mongodb-driver-probe --all-targets -- -D warnings
run_step probe-build cargo build --locked --offline --jobs 1 -p oo-mongodb-driver-probe --bin oo-w141-doc-arrow

export FROZEN_ARROW=/frozen/batch.arrow
export CANONICAL_JSON=/canonical/canonical.json
export OUT_ARROW=/evidence/shipments-doc.arrow
export OUT_IDENTITY=/evidence/arrow-identity.json
set +e
/target/debug/oo-w141-doc-arrow > /evidence/append.txt 2>&1
rc=$?
set -e
printf '%s\n' "$rc" > /evidence/append.exit
if [[ "$rc" -ne 0 ]]; then
  printf 'FAILED append %s\n' "$rc" > /evidence/status.txt
  exit "$rc"
fi
if [[ -n "${HOST_UID:-}" && -n "${HOST_GID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /evidence || true
fi
printf 'DONE\n' > /evidence/status.txt
