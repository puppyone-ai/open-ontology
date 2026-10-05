#!/bin/bash
# Overlay probe on a copy of immutable af3308c3. Rust oracle only. No Python.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_TERM_COLOR=never
export OO_A178_OUT=/evidence/out
cd /src
echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
uname -a
cat /etc/os-release
id
echo "=== product hashes ==="
sha256sum \
  Cargo.toml Cargo.lock rust-toolchain.toml \
  crates/oo-builder/Cargo.toml crates/oo-core/Cargo.toml crates/oo-jobs/Cargo.toml \
  crates/oo-builder/src/lib.rs crates/oo-builder/src/bin/oo-builder.rs \
  crates/oo-builder/src/role_pack.rs crates/oo-builder/src/role_pack_cli.rs \
  crates/oo-builder/tests/role_pack.rs crates/oo-builder/tests/role_pack_cli.rs \
  contracts/builder/role-pack-v1.md contracts/builder/role-pack-cli-v1.md \
  > /evidence/product-sha256.txt
if sha256sum -c /evidence/expect-sha256.txt > /evidence/hash-compare.txt 2>&1; then
  echo 0 > /evidence/hash-compare.exit
else
  echo "product hash mismatch" >&2
  echo 2 > /evidence/hash-compare.exit
  exit 2
fi
echo "=== overlay hash ==="
sha256sum crates/oo-builder/tests/role_pack_a178_review.rs > /evidence/overlay.sha256
if [ ! -s /evidence/overlay.sha256 ]; then
  echo "overlay missing" >&2
  exit 2
fi
echo "=== unsafe scan ==="
grep -n "unsafe" \
  crates/oo-builder/src/role_pack_cli.rs \
  crates/oo-builder/src/bin/oo-builder.rs \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/tests/role_pack_a178_review.rs \
  > /evidence/unsafe-lines.txt || true
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
  echo "not_run" > /evidence/author-test.exit
  echo "not_run" > /evidence/oracle.exit
  echo "not_run" > /evidence/clippy.exit
  exit "$fetch_exit"
fi
echo "=== author CLI and library tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test role_pack_cli --test structure_draft_cli --test captured_pg_cli --test review_pg_cli --test role_pack > /evidence/author-test.stdout 2> /evidence/author-test.stderr
author_exit=$?
set -e
echo "$author_exit" > /evidence/author-test.exit
echo "PHASE author_exit=$author_exit"
echo "=== independent oracle ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test role_pack_a178_review -- --nocapture > /evidence/oracle.stdout 2> /evidence/oracle.stderr
oracle_exit=$?
set -e
echo "$oracle_exit" > /evidence/oracle.exit
echo "PHASE oracle_exit=$oracle_exit"
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
component_exit=$?
set -e
echo "$component_exit" > /evidence/rustup-clippy.exit
echo "PHASE clippy_component_exit=$component_exit"
echo "=== clippy ==="
set +e
if [ "$component_exit" -ne 0 ]; then
  echo "clippy component missing" > /evidence/clippy.stderr
  clippy_exit=$component_exit
else
  cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
  clippy_exit=$?
fi
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
for phase_exit in "$fetch_exit" "$author_exit" "$oracle_exit" "$clippy_exit"; do
  if [ "$phase_exit" -gt "$worst" ]; then
    worst=$phase_exit
  fi
done
echo "$worst" > /evidence/worst.exit
echo "worst_phase_exit=$worst"
exit "$worst"
