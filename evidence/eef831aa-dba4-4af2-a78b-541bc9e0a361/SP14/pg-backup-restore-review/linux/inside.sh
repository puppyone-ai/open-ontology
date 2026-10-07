#!/bin/bash
# Compile and test the frozen tool and the independent oracle. No cargo update.
set -euo pipefail
mkdir -p /evidence/linux/build/phases /out
rustc --version > /evidence/linux/build/rustc-version.txt
cargo --version > /evidence/linux/build/cargo-version.txt
printf '%s\n' no > /evidence/linux/build/cargo-update-invoked.txt
rustc --version --verbose > /evidence/linux/build/rustc-verbose.txt

run_phase() {
  name=$1
  shift
  set +e
  "$@" > "/evidence/linux/build/${name}.log" 2>&1
  code=$?
  set -e
  printf '%s\n' "$code" > "/evidence/linux/build/phases/${name}.exit"
  if [ "$code" -ne 0 ]; then
    exit "$code"
  fi
}

run_phase clippy-component rustup component add clippy --toolchain 1.94.0
run_phase test cargo test --locked --offline --manifest-path /src/Cargo.toml
run_phase clippy cargo clippy --locked --offline --all-targets --manifest-path /src/Cargo.toml -- -D warnings
run_phase oracle-test cargo test --locked --offline --manifest-path /oracle/Cargo.toml
run_phase oracle-clippy cargo clippy --locked --offline --all-targets --manifest-path /oracle/Cargo.toml -- -D warnings
run_phase build cargo build --locked --offline --release --manifest-path /src/Cargo.toml
run_phase oracle-build cargo build --locked --offline --release --manifest-path /oracle/Cargo.toml
cp /target/review/release/pg-backup-restore /out/pg-backup-restore
cp /target/review/release/independent-gold /out/independent-gold
chmod 755 /out/pg-backup-restore /out/independent-gold
printf '%s\n' 0 > /evidence/linux/build/inside.exit
