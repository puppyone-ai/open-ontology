#!/bin/bash
# Eleventh proof. cargo test did not leave the bin on disk. Build the Linux bins only.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export TMPDIR=/target/tmp
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy NO_PROXY no_proxy
unset OPENAI_API_KEY OPENAI_BASE_URL OPENAI_API_BASE
mkdir -p /target/tmp /evidence/phases
cd /crate
rustc -vV | tee /evidence/rustc-version.txt
if ! grep -q '1.94.0' /evidence/rustc-version.txt; then
  echo "msrv mismatch" >&2
  exit 1
fi
set +e
cargo build --manifest-path /crate/Cargo.toml --locked --offline --bins > /evidence/build.stdout 2> /evidence/build.stderr
code=$?
printf '%s\n' "$code" > /evidence/phases/build.exit
echo "build_exit=${code}"
if [ "$code" -ne 0 ]; then
  exit "$code"
fi
ls -l /target/debug/oo-local-llm-models /target/debug/oo-local-llm-relay > /evidence/bins.txt
echo "proof_ok"
