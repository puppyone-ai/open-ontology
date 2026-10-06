#!/bin/bash
# Author 8 description-pack tests and Clippy on the sealed archive, then the
# independent overlay. The archive stays read-only. No lock generation.
# No Python oracle. No PostgreSQL.
# Origin shape: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a288/SP07/description-review-review/linux/proof-4/inside.sh
# content sha256 0f7ff79824026f1ee3b7a4a18eaa6ca27203f726caeeb4a9bbdfdc0e37f9ea62
# Change: description-pack is already in the fixed archive. Smoke is the author
# 8 tests plus Clippy before copying overlay/a302_sealed.rs. Lock subset is the
# a292 path-count-5 checker. Registry count stays 117.
set -u
set -e
set -o pipefail
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
printf '%s\n' no > /evidence/cargo-update-invoked.txt

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
echo "rustc_vv_exit=${PIPESTATUS[0]}"
cargo -V | tee /evidence/cargo-version.txt
echo "cargo_v_exit=${PIPESTATUS[0]}"
rustup show
uname -a
cat /etc/os-release
id
release=$(rustc -vV | awk '/^release: / { print $2 }')
printf '%s\n' "$release" > /evidence/rustc-release.txt
if [ "$release" != "1.94.0" ]; then
  echo "toolchain is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

rm -rf /work/tree
mkdir -p /work/tree
cp /src/Cargo.toml /src/Cargo.lock /work/tree/
if [ -f /src/rust-toolchain.toml ]; then
  cp /src/rust-toolchain.toml /work/tree/rust-toolchain.toml.aside
fi
printf '%s\n' moved-aside > /evidence/toolchain-aside.txt
cp -R /src/crates /work/tree/crates
cp -R /src/tools /work/tree/tools
find /work/tree -name '._*' -delete
find /work/tree -name '.DS_Store' -delete
cd /work/tree
if [ ! -f tools/description-pack/Cargo.lock ]; then
  echo "archive is missing the description-pack Cargo.lock" >&2
  printf '%s\n' missing > /evidence/lock.exit
  exit 2
fi
printf '%s\n' present > /evidence/lock.exit
if [ ! -f crates/oo-builder/src/description_candidate.rs ]; then
  echo "archive is missing description_candidate.rs" >&2
  exit 2
fi
sha256sum crates/oo-builder/src/description_candidate.rs > /evidence/description-candidate.sha256

cmp_one() {
  if ! cmp -s "$1" "$2"; then
    echo "blob mismatch $1" >&2
    printf '%s\n' 2 > /evidence/blob-cmp.exit
    exit 2
  fi
}
while IFS= read -r rel; do
  cmp_one "tools/description-pack/$rel" "/blobs/$rel"
done < <(cd /blobs && find . -type f ! -name '._*' ! -name '.DS_Store' | sed 's|^\./||' | sort)
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -f tools/description-pack/tests/a302_sealed.rs ] || [ -f /src/tools/description-pack/tests/a302_sealed.rs ]; then
  echo "overlay present before smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit

python_present=0
if command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
  python_present=1
fi
printf '%s\n' "$python_present" > /evidence/python-present.flag
echo "python_present_flag=$python_present"

find tools/description-pack tools/description-review -type f ! -name '._*' ! -name '.DS_Store' | sort > /evidence/product-files.txt
{
  sha256sum Cargo.lock crates/oo-builder/src/description_candidate.rs
  while IFS= read -r rel; do
    sha256sum "$rel"
  done < /evidence/product-files.txt
} > /evidence/src-before.txt

echo "=== registry triples ==="
set +e
rustc --edition 2024 -D warnings /tools/lock_subset.rs -o /usr/local/bin/lock_subset
lock_compile=$?
set -e
printf '%s\n' "$lock_compile" > /evidence/lock-compile.exit
if [ "$lock_compile" -ne 0 ]; then
  echo "lock_subset did not compile" >&2
  exit "$lock_compile"
fi
set +e
/usr/local/bin/lock_subset tools/description-pack/Cargo.lock /src/Cargo.lock > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
lock_exit=$?
set -e
printf '%s\n' "$lock_exit" > /evidence/lock-subset.exit
echo "PHASE lock_subset_exit=$lock_exit"
if ! grep -q 'summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true' /evidence/lock-subset.stdout; then
  echo "registry summary is not 117/0/0 path 5" >&2
  printf '%s\n' 2 > /evidence/lock-summary.exit
  lock_exit=2
fi
if [ ! -f /evidence/lock-summary.exit ]; then
  printf '%s\n' 0 > /evidence/lock-summary.exit
fi

echo "=== cargo fetch description-pack locked offline ==="
set +e
cargo fetch --manifest-path tools/description-pack/Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch description-pack locked online ==="
  set +e
  cargo fetch --manifest-path tools/description-pack/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/smoke-test.exit
  printf '%s\n' not_run > /evidence/clippy-pre.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== pre-overlay author cargo test ==="
set +e
cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test pack -- --test-threads=1 > /evidence/smoke-test.stdout 2> /evidence/smoke-test.stderr
smoke_exit=$?
set -e
printf '%s\n' "$smoke_exit" > /evidence/smoke-test.exit
echo "PHASE smoke_exit=$smoke_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/smoke-test.stdout > /evidence/smoke-count.txt
smoke_count=$(cat /evidence/smoke-count.txt)
echo "smoke_passed_count=$smoke_count"
missing_name=0
for name in \
  approved_text_is_resealed_and_packed_per_role \
  cli_matches_the_library \
  incomplete_compiler_does_not_leak_candidate_text \
  occupied_dataset_review_is_unsupported \
  reject_and_defer_leave_the_original_seal \
  review_hash_and_artifact_mismatch_refuse \
  stale_schema_and_cas_refuse_without_a_document \
  unclassified_allowlist_is_unknown_pii
do
  if ! grep -q "$name" /evidence/smoke-test.stdout; then
    echo "smoke missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
if grep -q 'a302_sealed' /evidence/smoke-test.stdout; then
  echo "overlay name appeared in the pre-overlay smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 8 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy-pre.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  if [ "$smoke_exit" -ne 0 ]; then
    exit "$smoke_exit"
  fi
  exit 1
fi

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy-pre.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$component_exit"
fi

echo "=== pre-overlay cargo clippy ==="
set +e
cargo clippy --manifest-path tools/description-pack/Cargo.toml --locked --offline --all-targets -- -D warnings > /evidence/clippy-pre.stdout 2> /evidence/clippy-pre.stderr
clippy_pre_exit=$?
set -e
printf '%s\n' "$clippy_pre_exit" > /evidence/clippy-pre.exit
echo "PHASE clippy_pre_exit=$clippy_pre_exit"
if [ "$clippy_pre_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$clippy_pre_exit"
fi

echo "=== overlay independent tests ==="
cp /overlay/a302_sealed.rs /work/tree/tools/description-pack/tests/a302_sealed.rs
if [ -f /src/tools/description-pack/tests/a302_sealed.rs ]; then
  echo "overlay leaked into the archive" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit
sha256sum tools/description-pack/tests/a302_sealed.rs > /evidence/overlay-sha256.txt

echo "=== independent cargo test ==="
set +e
cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test a302_sealed -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

echo "=== post-overlay cargo clippy ==="
set +e
cargo clippy --manifest-path tools/description-pack/Cargo.toml --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

{
  sha256sum Cargo.lock crates/oo-builder/src/description_candidate.rs
  while IFS= read -r rel; do
    sha256sum "$rel"
  done < /evidence/product-files.txt
} > /evidence/src-after.txt
if cmp -s /evidence/src-before.txt /evidence/src-after.txt; then
  printf '%s\n' 0 > /evidence/product-unchanged.exit
else
  printf '%s\n' 2 > /evidence/product-unchanged.exit
  echo "product bytes changed inside the container" >&2
fi
cp tools/description-pack/Cargo.lock /evidence/Cargo.lock
sha256sum tools/description-pack/Cargo.lock | awk '{print $1}' > /evidence/child-lock.sha256
sha256sum Cargo.lock | awk '{print $1}' > /evidence/root-lock.sha256
find tools/description-pack -type f ! -name '._*' ! -name '.DS_Store' ! -name 'a302_sealed.rs' | sort | xargs sha256sum > /evidence/pack-source.sha256

worst=0
if [ "$lock_exit" -ne 0 ]; then worst=$lock_exit; fi
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
if [ "$(cat /evidence/product-unchanged.exit)" -ne 0 ]; then worst=2; fi
echo "worst_phase_exit=$worst"
exit "$worst"
