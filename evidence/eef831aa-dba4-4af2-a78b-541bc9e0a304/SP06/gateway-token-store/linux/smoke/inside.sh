#!/bin/bash
# Focused MSRV smoke of the fixed archive before the gateway-token-store overlay.
# Uses the committed session-store lock. No generate-lockfile. No cargo update.
set -eu
echo "PHASE toolchain"
rustc --version
cargo --version
rustc --version | grep -q '1.94.0'
echo "PHASE tree"
if [ -e /src/tools/gateway-token-store ]; then
  echo "fixed archive already contains gateway-token-store" >&2
  exit 2
fi
test -f /src/tools/session-store/Cargo.toml
test -f /src/tools/session-store/Cargo.lock
echo "PHASE lock_recipe committed_session_store_lock fetch_locked check_locked_offline no_generate_lockfile no_cargo_update"
cd /src/tools/session-store
cargo fetch --locked
cargo check --locked --offline --lib
echo "PHASE smoke_ok"
