#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 process smoke.
# Immutable git archive of the fixed commit. The Pi spike is not in this tree.
# No PostgreSQL. No product edit. No Pi adapter.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
cd /src
echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
uname -a
cat /etc/os-release
id
echo "=== toolchain file ==="
cat rust-toolchain.toml || true
echo "=== preedit absence ==="
if [ -e testing/spikes/p09-pi-rpc ]; then
  echo "p09-pi-rpc is in the preedit archive" >&2
  echo 2 > /evidence/absence.exit
  exit 2
fi
if [ ! -f rust-toolchain.toml ]; then
  echo "fixed archive is missing rust-toolchain.toml" >&2
  echo 2 > /evidence/absence.exit
  exit 2
fi
echo 0 > /evidence/absence.exit
echo "=== archive hashes ==="
sha256sum rust-toolchain.toml > /evidence/archive-sha256.txt
if sha256sum -c /evidence/expect-sha256.txt > /evidence/hash-compare.txt 2>&1; then
  echo 0 > /evidence/hash-compare.exit
else
  echo "archive hash mismatch" >&2
  echo 2 > /evidence/hash-compare.exit
  exit 2
fi
echo "=== rustc process smoke ==="
set +e
rustc --edition 2024 -D warnings /evidence/process_smoke.rs -o /usr/local/bin/process_smoke \
  > /evidence/rustc.stdout 2> /evidence/rustc.stderr
rustc_exit=$?
set -e
echo "$rustc_exit" > /evidence/rustc.exit
echo "PHASE rustc_exit=$rustc_exit"
if [ "$rustc_exit" -ne 0 ]; then
  echo "not_run" > /evidence/smoke.exit
  echo "worst_phase_exit=$rustc_exit"
  exit "$rustc_exit"
fi
set +e
/usr/local/bin/process_smoke > /evidence/smoke.stdout 2> /evidence/smoke.stderr
smoke_exit=$?
set -e
echo "$smoke_exit" > /evidence/smoke.exit
echo "PHASE smoke_exit=$smoke_exit"
echo "worst_phase_exit=$smoke_exit"
exit "$smoke_exit"
