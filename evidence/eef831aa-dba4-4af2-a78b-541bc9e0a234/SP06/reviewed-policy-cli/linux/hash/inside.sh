#!/bin/bash
set -eu
rustc --edition 2021 /tools/sha256.rs -o /work/sha256
/work/sha256 dir /evidence/SHA256SUMS /evidence
