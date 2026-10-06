#!/bin/bash
# Overlay proof. One review test drives compiled init-pg, import-pg, and analyze-pg,
# then load_policy_catalog. Clippy follows. No open transaction is passed in.
# Origin: a241 overlay inside e74a8e68e368df08bfbfcbfc8c5de7a2945e89ef942e4d32cae41dd2e84368d4.
# Changes: a247 oracle, pipeline bins, no producer bundle, no timeout campaign.
set -u
echo "oo_task=sp04-analyze-pg-review-overlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a247"
echo "oo_fixed=6ac2216505723b51ae116b1938aca47846ade35f"
echo "oo_platform=linux/arm64"
echo "phase=overlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
export OO_A247_LIVE=1
unset OO_ANALYZE_PG_HOST || true
mkdir -p /evidence/linux/overlay/phases /evidence/live /cargo-home/bin /tmp/oo-a247-secrets
cd /src

record() {
  mkdir -p /evidence/linux/overlay/phases
  printf '%s\n' "$2" > "/evidence/linux/overlay/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/overlay/overlay.exit
    exit "$2"
  fi
}

if [ ! -f /evidence/linux/smoke/smoke.exit ] || [ "$(cat /evidence/linux/smoke/smoke.exit)" != "0" ]; then
  echo "pre-overlay smoke exit is not 0" >&2
  record smoke-gate 2
fi
record smoke-gate 0

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/overlay/rustc-version.txt
cargo -V | tee /evidence/linux/overlay/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
printf '%s\n' 0 > /evidence/linux/overlay/phases/toolchain.exit

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  record sha256-tool $?
else
  record sha256-tool 0
fi
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-gateway/tests/analyze_pg_a247_review.rs ]; then
  echo "review test is not mounted" >&2
  record source 2
fi
if [ ! -f crates/oo-pipeline/src/bin/analyze-pg.rs ]; then
  echo "analyze-pg is missing" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "overlay tree still selects a toolchain file" >&2
  record source 2
fi
if [ -e /run/oo-a247/fixture/server.key ] || [ -e /run/oo-a247/fixture/ca.key ] || [ -e /run/oo-a247/fixture/wrong.key ] || [ -e /run/oo-a247/fixture/server.csr ]; then
  echo "builder fixture contains a private key" >&2
  record fixture-keys 2
fi
find /run/oo-a247/fixture -type f -print | sort > /evidence/linux/overlay/fixture-files.txt
printf '%s\n' "overlay_mounted" > /evidence/linux/overlay/overlay-mounted.txt
record fixture-keys 0
record source 0

/cargo-home/bin/sha256 file \
  crates/oo-pipeline/src/bin/analyze-pg.rs \
  crates/oo-pipeline/tests/analyze_pg_cli.rs \
  contracts/pipeline/analyze-pg-cli-v1.md \
  crates/oo-pipeline/src/bin/init-pg.rs \
  crates/oo-pipeline/src/bin/oo-pipeline.rs \
  crates/oo-pipeline/src/target_statistics.rs \
  crates/oo-pipeline/src/import_cli.rs \
  crates/oo-pipeline/src/lib.rs \
  crates/oo-pipeline/Cargo.toml \
  crates/oo-gateway/src/policy.rs \
  crates/oo-gateway/src/policy_catalog.rs \
  crates/oo-gateway/src/lib.rs \
  Cargo.lock \
  > /evidence/linux/overlay/product-sha256.txt
record product-hash $?
while read -r hash path; do
  [ -n "$path" ] || continue
  if ! awk -v h="$hash" -v p="$path" '$1==h && $2==p {found=1} END {exit !found}' /evidence/linux/overlay/product-sha256.txt; then
    echo "frozen file changed $path" >&2
    record frozen-match 1
  fi
done < /evidence/linux/smoke/owned-sha256.txt
record frozen-match 0

libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a247-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a247-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a247-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a247-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a247-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/overlay/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  record libclang 1
fi
record libclang 0
printf '%s\n' "no" > /evidence/linux/overlay/cargo-update-invoked.txt

echo "=== cargo build pipeline bins ==="
set +e
cargo build --manifest-path Cargo.toml --locked --offline -p oo-pipeline --bin init-pg --bin oo-pipeline --bin analyze-pg \
  > /evidence/linux/overlay/bins-build.log 2>&1
bins_build=$?
set -e
record bins-build "$bins_build"
if [ ! -x /target/debug/init-pg ] || [ ! -x /target/debug/oo-pipeline ] || [ ! -x /target/debug/analyze-pg ]; then
  echo "compiled pipeline binary missing" >&2
  record bins 1
fi
record bins 0
/cargo-home/bin/sha256 file /target/debug/init-pg /target/debug/oo-pipeline /target/debug/analyze-pg \
  > /evidence/linux/overlay/bins-sha256.txt

export OO_A247_INIT_PG=/target/debug/init-pg
export OO_A247_PIPELINE=/target/debug/oo-pipeline
export OO_A247_ANALYZE=/target/debug/analyze-pg
export OO_A247_PG_HOST=oo-a247-pg
export OO_A247_PG_PORT=5432
export OO_A247_PG_ADMIN_USER=oo_a247_admin
export OO_A247_PG_ADMIN_PASSWORD_FILE=/run/oo-a247/fixture/admin.pass
export OO_A247_PG_CA_FILE=/run/oo-a247/fixture/ca.crt
export OO_A247_PG_WRONG_CA_FILE=/run/oo-a247/fixture/wrong-ca.crt
export OO_A247_EVIDENCE=/evidence/live

echo "=== cargo test review ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test analyze_pg_a247_review -- --test-threads=1 --nocapture \
  > /evidence/linux/overlay/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"
if ! grep -q 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' /evidence/linux/overlay/test.log; then
  echo "expected 1 review test" >&2
  record test-count 1
fi
oracle=/evidence/live/oracle.txt
if [ ! -f "$oracle" ]; then
  echo "live oracle was not written" >&2
  record oracle 1
fi
for fact in \
  'import_state=awaiting_publish' \
  'import_rows=2' \
  'reltuples_before=-1' \
  'sentinel_before=-1' \
  'wrong_ca=4' \
  'wrong_ca_fact=target connect' \
  'runtime=5' \
  'runtime_fact=runtime caller' \
  'plain=5' \
  'plain_fact=target role' \
  'stale=6' \
  'stale_fact=stale generation' \
  'absent=6' \
  'absent_fact=mapping absent' \
  'reltuples_after_refusal=-1' \
  'published=false' \
  'estimate_is_exact=false' \
  'count_rows=2' \
  'sentinel_after=-1' \
  'second_published=false' \
  'second_exact=false' \
  'admin_idle_in_transaction=0' \
  'maintenance=explicit' \
  'open_transaction_input=absent' \
  'timeout_campaign=not_run' \
  'forged_relation_campaign=not_run' \
  'source_audit_delta=0' \
  'source_reader_writes=0' \
  'source_reltuples=-1' \
  'acl_install=fixture' \
  'policy_catalog=loaded' \
  'unknown_rows=refuse' \
  'query_status=succeeded' \
  'query_count=2' \
  'statistics_published=false'
do
  if ! grep -qx "$fact" "$oracle"; then
    echo "oracle missing $fact" >&2
    record oracle 1
  fi
done
if ! grep -q '^disposition=\(reusable\|fresh_connection\)$' "$oracle"; then
  echo "oracle missing disposition" >&2
  record oracle 1
fi
if ! grep -q '^estimated_rows=' "$oracle" || ! grep -q '^catalog_estimated_rows=' "$oracle"; then
  echo "oracle missing estimate" >&2
  record oracle 1
fi
est=$(awk -F= '$1=="estimated_rows" {print $2}' "$oracle")
cat_est=$(awk -F= '$1=="catalog_estimated_rows" {print $2}' "$oracle")
if [ -z "$est" ] || [ "$est" != "$cat_est" ]; then
  echo "catalog estimate does not match the receipt" >&2
  record oracle 1
fi
record oracle 0
record test-count 0

leak=0
for facts in /evidence/live/oracle.txt /evidence/live/analyze.stdout /evidence/linux/overlay/test.log; do
  if [ ! -f "$facts" ]; then
    continue
  fi
  for secret_file in /run/oo-a247/fixture/*.pass /tmp/oo-a247-secrets/*.pass; do
    [ -f "$secret_file" ] || continue
    secret=$(tr -d '[:space:]' < "$secret_file")
    if [ -n "$secret" ] && grep -q -F "$secret" "$facts"; then
      echo "password text appeared in evidence" >&2
      leak=1
    fi
  done
done
record secret-scan "$leak"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/overlay/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/overlay/clippy-component.log
  record clippy-component 0
fi
echo "=== cargo clippy analyze-pg ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-pipeline --bin analyze-pg --test analyze_pg_cli -- -D warnings \
  > /evidence/linux/overlay/clippy-pipeline.log 2>&1
clippy_pipeline=$?
set -e
record clippy-pipeline "$clippy_pipeline"
echo "=== cargo clippy review ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline \
  -p oo-gateway --test analyze_pg_a247_review -- -D warnings \
  > /evidence/linux/overlay/clippy-review.log 2>&1
clippy_review=$?
set -e
record clippy-review "$clippy_review"
if [ "$clippy_pipeline" -eq 0 ] && [ "$clippy_review" -eq 0 ]; then
  printf '%s\n' 0 > /evidence/linux/overlay/clippy.exit
else
  printf '%s\n' 1 > /evidence/linux/overlay/clippy.exit
  record clippy 1
fi
printf '%s\n' 0 > /evidence/linux/overlay/phases/clippy.exit

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/overlay/lock-after.sha256
before=$(awk '$2=="Cargo.lock" {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/overlay/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/overlay/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/overlay/lock-hash.txt
record lock-hash 0

if grep -nE 'unsafe[[:space:]{]' \
  crates/oo-pipeline/src/bin/analyze-pg.rs \
  crates/oo-gateway/tests/analyze_pg_a247_review.rs \
  > /evidence/linux/overlay/unsafe-lines.txt
then
  echo "review tree contains an unsafe block" >&2
  record unsafe 2
fi
printf '%s\n' "no unsafe block" > /evidence/linux/overlay/unsafe-lines.txt
printf '%s\n' 0 > /evidence/linux/overlay/phases/unsafe.exit

/cargo-home/bin/sha256 file crates/oo-gateway/tests/analyze_pg_a247_review.rs \
  > /evidence/linux/overlay/review-test-sha256.txt

printf '%s\n' 0 > /evidence/linux/overlay/overlay.exit
echo "overlay_proof=ok"
exit 0
