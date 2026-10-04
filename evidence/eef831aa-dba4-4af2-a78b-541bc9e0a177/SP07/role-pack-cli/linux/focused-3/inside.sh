#!/bin/bash
# Focused Linux arm64 Rust 1.94 rerun after the scale-column name collision fix.
# linux/focused and linux/focused-2 stay preserved. No Python. No PostgreSQL.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export OO_A177_OUT=/evidence/out
cd /src
echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
id
echo "=== archive hashes ==="
sha256sum \
  Cargo.lock \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/bin/oo-builder.rs \
  crates/oo-builder/src/role_pack.rs \
  crates/oo-builder/src/role_pack_cli.rs \
  crates/oo-builder/tests/role_pack_cli.rs \
  contracts/builder/role-pack-v1.md \
  contracts/builder/role-pack-cli-v1.md \
  > /evidence/archive-sha256.txt
if sha256sum -c /evidence/expect-sha256.txt > /evidence/hash-compare.txt 2>&1; then
  echo 0 > /evidence/hash-compare.exit
else
  echo "archive hash mismatch" >&2
  echo 2 > /evidence/hash-compare.exit
  exit 2
fi
echo "=== unsafe scan ==="
grep -n "unsafe" \
  crates/oo-builder/src/role_pack_cli.rs \
  crates/oo-builder/src/bin/oo-builder.rs \
  crates/oo-builder/src/lib.rs \
  > /evidence/unsafe-lines.txt || true
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
clippy_component_exit=$?
set -e
echo "$clippy_component_exit" > /evidence/rustup-clippy.exit
echo "PHASE clippy_component_exit=$clippy_component_exit"
mkdir -p /evidence/out
echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
echo "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
echo "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  echo "not_run" > /evidence/test.exit
  echo "not_run" > /evidence/clippy.exit
  exit "$fetch_exit"
fi
echo "=== cargo test builder CLI and role pack ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test role_pack_cli --test structure_draft_cli --test captured_pg_cli --test review_pg_cli --test role_pack > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
echo "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
echo "=== cargo clippy oo-builder all targets ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
echo "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
{
  echo "rustc_vv_exit=0"
  rustc -vV
  echo "cargo_v_exit=0"
  cargo -V
} > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt
worst=0
for phase_exit in "$clippy_component_exit" "$fetch_exit" "$test_exit" "$clippy_exit"; do
  if [ "$phase_exit" -gt "$worst" ]; then
    worst=$phase_exit
  fi
done
echo "$worst" > /evidence/worst.exit
echo "worst_phase_exit=$worst"
exit "$worst"
