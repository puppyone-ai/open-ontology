#!/bin/bash
# Focused oo-cli smoke inside the pinned linux/arm64 Rust image.
# Fetch is online. Test and Clippy are locked and offline. Not an air-gap claim.
set -u
export HOME=/tmp
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo
export CARGO_TARGET_DIR=/target
mkdir -p /out/phases
log() { printf '%s\n' "$*" | tee -a /out/console.log >/dev/null; }

cd /src
log "snapshot_cargo_toml=$([ -f Cargo.toml ] && echo present || echo missing)"
log "msrv_override=RUSTUP_TOOLCHAIN=1.94.0"
log "airgap_claim=no"
log "network_phase=cargo_fetch_online"
log "offline_phase=test_clippy"
log "package=oo-cli"
log "input=head archive plus owned oo-cli overlay"

log "=== rustc ==="
rustc -vV > /out/phases/rustc.txt 2>&1
rustc_ec=$?
printf '%s\n' "$rustc_ec" > /out/phases/rustc.exit
log "phase_exit=$rustc_ec phase=rustc"

log "=== cargo ==="
cargo -V > /out/phases/cargo.txt 2>&1
cargo_ec=$?
printf '%s\n' "$cargo_ec" > /out/phases/cargo.exit
log "phase_exit=$cargo_ec phase=cargo"

log "=== clippy-component ==="
rustup component add clippy --toolchain 1.94.0 > /out/phases/clippy-component.txt 2>&1
clip_ec=$?
printf '%s\n' "$clip_ec" > /out/phases/clippy-component.exit
log "phase_exit=$clip_ec phase=clippy-component"

log "=== fetch ==="
cargo fetch --manifest-path Cargo.toml --locked > /out/phases/fetch.txt 2>&1
fetch_ec=$?
printf '%s\n' "$fetch_ec" > /out/phases/fetch.exit
log "phase_exit=$fetch_ec phase=fetch"
if [ "$fetch_ec" -ne 0 ]; then
  log "offline_phases=not_run"
  log "elapsed_seconds=$SECONDS"
  exit "$fetch_ec"
fi

log "=== test ==="
cargo test --manifest-path Cargo.toml --locked --offline -p oo-cli > /out/phases/test.txt 2>&1
test_ec=$?
printf '%s\n' "$test_ec" > /out/phases/test.exit
log "phase_exit=$test_ec phase=test"

log "=== clippy ==="
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-cli --all-targets -- -D warnings > /out/phases/clippy.txt 2>&1
clippy_ec=$?
printf '%s\n' "$clippy_ec" > /out/phases/clippy.exit
log "phase_exit=$clippy_ec phase=clippy"

worst=0
if [ "$test_ec" -ne 0 ]; then worst=$test_ec; fi
if [ "$clippy_ec" -ne 0 ]; then worst=$clippy_ec; fi
printf '%s\n' "$worst" > /out/phases/worst.exit
log "worst_phase_exit=$worst"
log "elapsed_seconds=$SECONDS"
exit "$worst"
