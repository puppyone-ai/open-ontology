#!/bin/bash
# Pre-overlay Linux arm64 Rust 1.94 smoke of fixed archive a48db85.
# The analyze-pg sources must be absent. Clippy of the new bin waits for the overlay.
# Origin: a204 preoverlay inside b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636
# via a223 e93611efa10954ffffc2190c20d526e77349991c3d48053a49a2627d4213bb9a.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
cd /src

echo "=== identity ==="
rustc +1.94.0 -vV | tee /evidence/rustc-version.txt
echo "rustc_vv_exit=${PIPESTATUS[0]}"
cargo +1.94.0 -V | tee /evidence/cargo-version.txt
echo "cargo_v_exit=${PIPESTATUS[0]}"
uname -a | tee /evidence/uname.txt
printf '%s\n' no > /evidence/cargo-update-invoked.txt

if ! grep -q 'release: 1.94.0' /evidence/rustc-version.txt; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/rustc.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/rustc.exit

echo "=== archive pins ==="
if [ -f crates/oo-pipeline/src/bin/analyze-pg.rs ] \
  || [ -f crates/oo-pipeline/tests/analyze_pg_cli.rs ] \
  || [ -f contracts/pipeline/analyze-pg-cli-v1.md ]
then
  echo "analyze-pg overlay present during smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
sha256sum \
  crates/oo-pipeline/src/target_statistics.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/src/bin/init-pg.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/Cargo.toml \
  Cargo.toml \
  Cargo.lock \
  > /evidence/archive-sha256.txt
sha256sum Cargo.lock > /evidence/lock-before.txt

echo "=== cargo fetch locked offline ==="
set +e
cargo +1.94.0 fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo +1.94.0 fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/lib.exit
  printf '%s\n' not_run > /evidence/cli.exit
  exit "$fetch_exit"
fi

echo "=== cargo test target_statistics ==="
set +e
cargo +1.94.0 test --manifest-path Cargo.toml --locked --offline -p oo-pipeline --lib target_statistics \
  -- --test-threads=1 \
  > /evidence/lib.stdout 2> /evidence/lib.stderr
lib_exit=$?
set -e
printf '%s\n' "$lib_exit" > /evidence/lib.exit
echo "PHASE lib_exit=$lib_exit"

echo "=== cargo test existing cli surfaces without ignored live ==="
set +e
cargo +1.94.0 test --manifest-path Cargo.toml --locked --offline -p oo-pipeline \
  --test target_statistics_pg_live --test init_pg_cli --test import_pg_cli_live \
  -- --test-threads=1 \
  > /evidence/cli.stdout 2> /evidence/cli.stderr
cli_exit=$?
set -e
printf '%s\n' "$cli_exit" > /evidence/cli.exit
echo "PHASE cli_exit=$cli_exit"

sha256sum Cargo.lock > /evidence/lock-after.txt
if ! cmp -s /evidence/lock-before.txt /evidence/lock-after.txt; then
  echo "Cargo.lock changed during smoke" >&2
  printf '%s\n' 2 > /evidence/lock-unchanged.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-unchanged.exit
printf '%s\n' deferred_until_overlay > /evidence/clippy.exit

worst=0
if [ "$lib_exit" -ne 0 ]; then worst=$lib_exit; fi
if [ "$cli_exit" -ne 0 ]; then worst=$cli_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
