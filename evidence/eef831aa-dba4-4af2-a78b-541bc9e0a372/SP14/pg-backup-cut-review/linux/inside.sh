#!/bin/bash
# Compile and test the backup tool and the independent cut checker. No cargo update.
# Does not run the unchanged 101-check oracle.
set -euo pipefail
mkdir -p /evidence/linux/build/phases /out
rustc --version > /evidence/linux/build/rustc-version.txt
cargo --version > /evidence/linux/build/cargo-version.txt
printf '%s\n' no > /evidence/linux/build/cargo-update-invoked.txt
rustc --version --verbose > /evidence/linux/build/rustc-verbose.txt
printf '%s\n' 0 > /evidence/linux/build/oracle-101-rerun.txt

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
run_phase check-test cargo test --locked --offline --manifest-path /check/Cargo.toml
run_phase check-clippy cargo clippy --locked --offline --all-targets --manifest-path /check/Cargo.toml -- -D warnings
run_phase build cargo build --locked --offline --release --manifest-path /src/Cargo.toml
run_phase check-build cargo build --locked --offline --release --manifest-path /check/Cargo.toml
cp /target/review/release/pg-backup-restore /out/pg-backup-restore
cp /target/review/release/cut-check /out/cut-check
chmod 755 /out/pg-backup-restore /out/cut-check
printf '%s\n' 0 > /evidence/linux/build/inside.exit
