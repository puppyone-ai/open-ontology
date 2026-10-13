#!/bin/bash
# Linux Rust 1.94 compile of the mounted mongodb-preview crate. Jobs 1.
set -euo pipefail
cd /work/tools/mongodb-preview
mkdir -p /evidence/src
cp Cargo.toml src/lib.rs src/main.rs /evidence/src/
rustc --version | tee /evidence/rustc-version.txt
cargo --version | tee /evidence/cargo-version.txt
grep -q '^rustc 1.94.0 ' /evidence/rustc-version.txt
if [[ -f Cargo.lock ]]; then
  cp Cargo.lock /evidence/Cargo.lock.before
fi
cargo generate-lockfile
cp Cargo.lock /evidence/Cargo.lock
cargo build --jobs 1 --locked --bins --lib
if [[ -n "${HOST_UID:-}" && -n "${HOST_GID:-}" ]]; then
  chown -R "${HOST_UID}:${HOST_GID}" /work/tools/mongodb-preview /evidence
fi
