#!/bin/bash
set -eu
mkdir -p /evidence/raw /run/fe01 /work/tree
rm -f /evidence/stop /evidence/ready.json /evidence/subject.request /evidence/subject.applied
rm -rf /work/tree
mkdir -p /work/tree
cp -R /snapshot/tools/control-identity/. /work/tree/
mkdir -p /work/tree/proof-bin
cp /evidence/harness/main.rs /work/tree/proof-bin/main.rs
printf '\n[[bin]]\nname = "fe01-b01-hold"\npath = "proof-bin/main.rs"\n' >> /work/tree/Cargo.toml
cd /work/tree
cargo build --locked --bin control-identity --bin fe01-b01-hold
export CONTROL_IDENTITY_BIN=/work/target/debug/control-identity
exec /work/target/debug/fe01-b01-hold
