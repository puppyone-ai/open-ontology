#!/bin/bash
set -euo pipefail
rustc --version > /evidence/linux/smoke/rustc-version.txt
cargo --version > /evidence/linux/smoke/cargo-version.txt
uname -m > /evidence/linux/smoke/uname.txt
printf '%s\n' 0 > /evidence/linux/smoke/rustc.exit
printf '%s\n' 0 > /evidence/linux/smoke/cargo.exit
