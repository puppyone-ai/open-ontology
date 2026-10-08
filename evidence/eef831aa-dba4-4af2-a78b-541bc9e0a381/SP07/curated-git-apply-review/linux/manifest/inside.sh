#!/bin/bash
# SHA-256 manifest. No cargo fetch. The output path is excluded by the tool.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
rustc +1.94.0 --edition 2021 -C debuginfo=0 /tool/sha256_manifest.rs -o /work/sha256_manifest > /evidence/rustc.stdout 2> /evidence/rustc.stderr
printf '%s\n' 0 > /evidence/rustc.exit
/work/sha256_manifest /review /review/MANIFEST.sha256 > /evidence/manifest.stdout 2> /evidence/manifest.stderr
printf '%s\n' 0 > /evidence/manifest.exit
chown -R 501:20 /evidence /review/MANIFEST.sha256
