#!/bin/bash
# Second focused run. Same toolchain and volumes. Adds the read-only
# golden tree and the TLS fixtures the dialogue tests include.
set -u
OUT=/evidence/linux/focused-2
PHASE=$OUT/phases
mkdir -p "$PHASE"
cd /work

export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1

rustc --version > "$OUT/rustc-version.txt" 2>&1
printf '%s\n' $? > "$PHASE/rustc.exit"
cargo --version > "$OUT/cargo-version.txt" 2>&1
printf '%s\n' $? > "$PHASE/cargo.exit"
uname -m > "$OUT/uname.txt"
printf '%s\n' no > "$OUT/cargo-update-invoked.txt"
if ! grep -q '1.94.0' "$OUT/rustc-version.txt"; then
  printf '%s\n' 94 > "$PHASE/toolchain.exit"
  exit 94
fi
printf '%s\n' 0 > "$PHASE/toolchain.exit"
cargo clippy --version > "$OUT/clippy-version.txt" 2>&1
printf '%s\n' 0 > "$PHASE/clippy-component.exit"

set +e
cargo test --manifest-path /work/Cargo.toml --locked --offline -p oo-builder --lib -- --test-threads=1 > "$OUT/test-lib.log" 2>&1
printf '%s\n' $? > "$PHASE/test-lib.exit"
cargo test --manifest-path /work/Cargo.toml --locked --offline -p oo-builder --test pack_dialogue_compat --test role_pack --test reviewed_effective -- --test-threads=1 > "$OUT/test-builder.log" 2>&1
printf '%s\n' $? > "$PHASE/test-builder.exit"
cargo test --manifest-path /work/tools/ontology-dialogue/Cargo.toml --locked --offline --test identity_compat --test propose --test execute_approval -- --test-threads=1 > "$OUT/test-dialogue.log" 2>&1
printf '%s\n' $? > "$PHASE/test-dialogue.exit"
cargo clippy --manifest-path /work/Cargo.toml --locked --offline -p oo-builder --lib --test pack_dialogue_compat -- -D warnings > "$OUT/clippy-builder.log" 2>&1
printf '%s\n' $? > "$PHASE/clippy-builder.exit"
cargo clippy --manifest-path /work/tools/ontology-dialogue/Cargo.toml --locked --offline --all-targets -- -D warnings > "$OUT/clippy-dialogue.log" 2>&1
printf '%s\n' $? > "$PHASE/clippy-dialogue.exit"
set -e

worst=0
for name in toolchain rustc cargo clippy-component test-lib test-builder test-dialogue clippy-builder clippy-dialogue; do
  code=$(cat "$PHASE/${name}.exit")
  if [ "$code" -gt "$worst" ]; then
    worst=$code
  fi
done
printf '%s\n' "$worst" > "$PHASE/worst.exit"
printf '%s\n' "$worst" > "$OUT/focused.exit"
exit "$worst"
