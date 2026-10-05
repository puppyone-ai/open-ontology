#!/bin/bash
# Compile the std-only manifest tool onto the private cargo volume. Do not hash here.
set -eu
mkdir -p /cargo-home/bin /cargo-home/tmp
export TMPDIR=/cargo-home/tmp
export HOME=/tmp
rustc --version
rustc --edition 2021 -D warnings -O -o /cargo-home/bin/oo-a201-sha256 /evidence/tools/sha256.rs
test -x /cargo-home/bin/oo-a201-sha256
/cargo-home/bin/oo-a201-sha256 file /evidence/tools/sha256.rs
echo phase_exit=0
