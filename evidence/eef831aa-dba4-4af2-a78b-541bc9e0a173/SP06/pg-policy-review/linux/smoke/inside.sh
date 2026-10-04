#!/bin/bash
# Pre-overlay Linux 1.94 smoke. The archive has no review test.
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
rustup show
uname -a
cat /etc/os-release
id
echo "=== toolchain file ==="
cat rust-toolchain.toml || true
echo "=== archive hashes ==="
sha256sum -c /evidence/expect-sha256.txt > /evidence/hash-compare.txt 2>&1
hash_exit=$?
echo "$hash_exit" > /evidence/hash-compare.exit
echo "PHASE hash_exit=$hash_exit"
if [ "$hash_exit" -ne 0 ]; then
  exit "$hash_exit"
fi
echo "=== overlay absent ==="
if [ -e crates/oo-gateway/tests/policy_pg_a173_review.rs ]; then
  echo "review overlay is in the smoke archive" >&2
  echo 2 > /evidence/overlay-absent.exit
  exit 2
fi
echo 0 > /evidence/overlay-absent.exit
echo "=== unsafe and taxonomy ==="
grep -n "unsafe" crates/oo-gateway/src/policy.rs crates/oo-gateway/src/lib.rs > /evidence/unsafe-lines.txt || true
if grep -n "unsafe" crates/oo-gateway/src/policy.rs >/dev/null; then
  echo 2 > /evidence/taxonomy.exit
  echo "policy.rs contains unsafe" >&2
  exit 2
fi
if grep -n "tokio-postgres-rustls" crates/oo-arch-check/src/policy.rs > /evidence/taxonomy-adapter-hit.txt; then
  echo 2 > /evidence/taxonomy.exit
  echo "architecture allow-list names tokio-postgres-rustls" >&2
  exit 2
fi
: > /evidence/taxonomy-adapter-hit.txt
if ! grep -n "tokio-postgres-rustls" crates/oo-gateway/Cargo.toml > /evidence/taxonomy-gateway-dep.txt; then
  echo 2 > /evidence/taxonomy.exit
  echo "gateway manifest has no tokio-postgres-rustls" >&2
  exit 2
fi
awk 'NR>=135 && NR<=140 {print NR":"$0}' crates/oo-arch-check/src/policy.rs > /evidence/taxonomy-stack.txt
echo 0 > /evidence/taxonomy.exit
echo "=== clippy component ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/rustup-clippy.stdout 2> /evidence/rustup-clippy.stderr
clippy_component_exit=$?
set -e
echo "$clippy_component_exit" > /evidence/rustup-clippy.exit
echo "PHASE clippy_component_exit=$clippy_component_exit"
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
  echo 1 > /evidence/test.exit
  echo 1 > /evidence/clippy.exit
  echo 1 > /evidence/arch.exit
  echo 1 > /evidence/crate-hash.exit
  echo "$fetch_exit" > /evidence/worst.exit
  exit "$fetch_exit"
fi
echo "=== crate byte checksums ==="
set +e
: > /evidence/crate-hash.txt
crate_bad=0
while IFS="$(printf '\t')" read -r name version checksum; do
  [ -n "$name" ] || continue
  found=$(find /cargo-home/registry/cache -name "${name}-${version}.crate" -type f 2>/dev/null | head -n 1)
  if [ -z "$found" ]; then
    echo "missing ${name} ${version}" >> /evidence/crate-hash.txt
    crate_bad=1
    continue
  fi
  got=$(sha256sum "$found" | awk '{print $1}')
  if [ "$got" = "$checksum" ]; then
    echo "match ${name} ${version}" >> /evidence/crate-hash.txt
  else
    echo "mismatch ${name} ${version}" >> /evidence/crate-hash.txt
    crate_bad=1
  fi
done < /evidence/crate-checksums.tsv
set -e
echo "$crate_bad" > /evidence/crate-hash.exit
echo "PHASE crate_hash_exit=$crate_bad"
echo "=== cargo clippy oo-gateway lib ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
echo "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
echo "=== cargo test oo-gateway lib ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
echo "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
echo "=== architecture tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-arch-check --lib -- gateway_postgres_stack_is_allowed gateway_sync_postgres_and_sqlx_are_forbidden dev_only_gateway_pipeline_is_forbidden > /evidence/arch.stdout 2> /evidence/arch.stderr
arch_exit=$?
set -e
echo "$arch_exit" > /evidence/arch.exit
echo "PHASE arch_exit=$arch_exit"
{
  echo "rustc_vv_exit=0"
  rustc -vV
  echo "cargo_v_exit=0"
  cargo -V
} > /evidence/rustc-version.txt
cargo -V > /evidence/cargo-version.txt
worst=0
for phase_exit in "$clippy_component_exit" "$fetch_exit" "$crate_bad" "$clippy_exit" "$test_exit" "$arch_exit"; do
  if [ "$phase_exit" -gt "$worst" ]; then
    worst=$phase_exit
  fi
done
echo "$worst" > /evidence/worst.exit
echo "worst_phase_exit=$worst"
exit "$worst"
