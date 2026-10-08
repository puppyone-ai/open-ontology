#!/bin/bash
set -u
OUT=/evidence/linux/focused-3
mkdir -p "$OUT/phases"
export HOME=/tmp CARGO_HOME=/cargo-home CARGO_TARGET_DIR=/target RUSTUP_TOOLCHAIN=1.94.0 CARGO_BUILD_JOBS=1
rustc --version > "$OUT/rustc-version.txt"
printf '%s\n' no > "$OUT/cargo-update-invoked.txt"
rustup component add clippy --toolchain 1.94.0 > "$OUT/clippy-component.log" 2>&1
printf '%s\n' $? > "$OUT/phases/clippy-component.exit"
cargo clippy --version > "$OUT/clippy-version.txt" 2>&1
set +e
cargo clippy --manifest-path /work/tools/ontology-dialogue/Cargo.toml --locked --offline --all-targets -- -D warnings > "$OUT/clippy-dialogue.log" 2>&1
printf '%s\n' $? > "$OUT/phases/clippy-dialogue.exit"
set -e
code=$(cat "$OUT/phases/clippy-dialogue.exit")
comp=$(cat "$OUT/phases/clippy-component.exit")
worst=$code
if [ "$comp" -gt "$worst" ]; then worst=$comp; fi
printf '%s\n' "$worst" > "$OUT/focused.exit"
exit "$worst"
