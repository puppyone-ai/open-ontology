#!/bin/bash
# Author 14 description-pack tests and Clippy on the sealed archive, then the
# unchanged a302 boundary and the a315 oracle. The archive stays read-only.
# No lock generation. No Python oracle. No PostgreSQL.
# Origin shape: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a302/SP07/description-pack-review/linux/proof-1/inside.sh
# content sha256 7b50a26ef4fc98859e6698e197342ab2391d3243eb00f0de04697f51a1583e95
# Change: the fixed archive already contains the 14-test implementation, so the
# pre-overlay smoke is those 14 tests plus Clippy. The overlay adds test files
# only: a302_sealed.rs from its reviewed path, and a315_sealed.rs.
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
A302_SHA=bf043d4cecc1a23d1ced7fdb75a4a139bcf69bffa8058858f2d33bffab18d7b1
CHILD_LOCK_SHA=2d1a39b212f23ce27461c1ada0b8f8406de538b2fd781d37f0d9674246a8619d
ROOT_LOCK_SHA=16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a

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
if [ -f tools/description-pack/tests/a302_sealed.rs ] || [ -f tools/description-pack/tests/a315_sealed.rs ] || [ -f /src/tools/description-pack/tests/a302_sealed.rs ] || [ -f /src/tools/description-pack/tests/a315_sealed.rs ]; then
  echo "overlay present before smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit

child_lock=$(sha256sum tools/description-pack/Cargo.lock | awk '{print $1}')
root_lock=$(sha256sum Cargo.lock | awk '{print $1}')
printf '%s\n' "$child_lock" > /evidence/child-lock.sha256
printf '%s\n' "$root_lock" > /evidence/root-lock.sha256
if [ "$child_lock" != "$CHILD_LOCK_SHA" ] || [ "$root_lock" != "$ROOT_LOCK_SHA" ]; then
  echo "lock content hash drifted" >&2
  printf '%s\n' 2 > /evidence/lock-content.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/lock-content.exit

python_present=0
if command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
  python_present=1
fi
printf '%s\n' "$python_present" > /evidence/python-present.flag
echo "python_present_flag=$python_present"

find tools/description-pack tools/description-review crates/oo-builder/src crates/oo-core/src -type f ! -name '._*' ! -name '.DS_Store' | sort > /evidence/product-files.txt
{
  sha256sum Cargo.toml Cargo.lock
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
  printf '%s\n' not_run > /evidence/a302-test.exit
  printf '%s\n' not_run > /evidence/a315-test.exit
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
  unclassified_allowlist_is_unknown_pii \
  occupied_override_without_authorization_stays_unsupported \
  occupied_override_approve_reseals_two_roles \
  occupied_cas_conflict_preserves_the_seal \
  occupied_accept_is_not_rewritten \
  occupied_stale_override_is_refused \
  occupied_defer_and_reject_leave_structural_overrides
do
  if ! grep -q "$name" /evidence/smoke-test.stdout; then
    echo "smoke missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
if grep -q 'a302_sealed' /evidence/smoke-test.stdout || grep -q 'a315_sealed' /evidence/smoke-test.stdout; then
  echo "overlay name appeared in the pre-overlay smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 14 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy-pre.exit
  printf '%s\n' not_run > /evidence/a302-test.exit
  printf '%s\n' not_run > /evidence/a315-test.exit
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
  printf '%s\n' not_run > /evidence/a302-test.exit
  printf '%s\n' not_run > /evidence/a315-test.exit
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
  printf '%s\n' not_run > /evidence/a302-test.exit
  printf '%s\n' not_run > /evidence/a315-test.exit
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$clippy_pre_exit"
fi

echo "=== overlay independent tests ==="
a302_got=$(sha256sum /a302-overlay/a302_sealed.rs | awk '{print $1}')
printf '%s\n' "$a302_got" > /evidence/a302-overlay.sha256
if [ "$a302_got" != "$A302_SHA" ]; then
  echo "a302 overlay hash drifted" >&2
  printf '%s\n' 2 > /evidence/a302-overlay.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/a302-overlay.exit
cp /a302-overlay/a302_sealed.rs /work/tree/tools/description-pack/tests/a302_sealed.rs
cp /overlay/a315_sealed.rs /work/tree/tools/description-pack/tests/a315_sealed.rs
if [ -f /src/tools/description-pack/tests/a302_sealed.rs ] || [ -f /src/tools/description-pack/tests/a315_sealed.rs ]; then
  echo "overlay leaked into the archive" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit
sha256sum tools/description-pack/tests/a302_sealed.rs tools/description-pack/tests/a315_sealed.rs > /evidence/overlay-sha256.txt

echo "=== a302 boundary cargo test ==="
set +e
cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test a302_sealed -- --test-threads=1 > /evidence/a302-test.stdout 2> /evidence/a302-test.stderr
a302_exit=$?
set -e
printf '%s\n' "$a302_exit" > /evidence/a302-test.exit
echo "PHASE a302_test_exit=$a302_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/a302-test.stdout > /evidence/a302-count.txt
a302_missing=0
for name in \
  approved_text_reseals_through_the_compiler \
  compiled_cli_matches_the_library \
  hash_tuple_cas_and_stale_field_refuse \
  incomplete_body_does_not_leak \
  occupied_dataset_review_is_unsupported \
  rejected_and_deferred_leave_the_original_seal \
  unclassified_allowlist_is_unknown_pii
do
  if ! grep -q "$name" /evidence/a302-test.stdout; then
    echo "a302 missing $name" >&2
    a302_missing=1
  fi
done
printf '%s\n' "$a302_missing" > /evidence/a302-names.exit
a302_count=$(cat /evidence/a302-count.txt)
if [ "$a302_exit" -eq 0 ] && { [ "$a302_count" -ne 7 ] || [ "$a302_missing" -ne 0 ]; }; then
  a302_exit=1
  printf '%s\n' 1 > /evidence/a302-test.exit
fi

echo "=== a315 oracle cargo test ==="
set +e
cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test a315_sealed -- --test-threads=1 > /evidence/a315-test.stdout 2> /evidence/a315-test.stderr
a315_exit=$?
set -e
printf '%s\n' "$a315_exit" > /evidence/a315-test.exit
echo "PHASE a315_test_exit=$a315_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/a315-test.stdout > /evidence/a315-count.txt
a315_missing=0
for name in \
  occupied_override_reseals_approved_fields_only \
  flag_absent_and_cas_refusals_keep_the_prior_seal \
  accept_and_stale_fail_closed \
  reject_and_defer_leave_structural_fields \
  mixed_fresh_and_occupied_approvals_reseal_once
do
  if ! grep -q "$name" /evidence/a315-test.stdout; then
    echo "a315 missing $name" >&2
    a315_missing=1
  fi
done
printf '%s\n' "$a315_missing" > /evidence/a315-names.exit
a315_count=$(cat /evidence/a315-count.txt)
if [ "$a315_exit" -eq 0 ] && { [ "$a315_count" -ne 5 ] || [ "$a315_missing" -ne 0 ]; }; then
  a315_exit=1
  printf '%s\n' 1 > /evidence/a315-test.exit
fi
if [ "$a302_exit" -ne 0 ]; then
  printf '%s\n' "$a302_exit" > /evidence/test.exit
else
  printf '%s\n' "$a315_exit" > /evidence/test.exit
fi

echo "=== post-overlay cargo clippy ==="
set +e
cargo clippy --manifest-path tools/description-pack/Cargo.toml --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

rm -f tools/description-pack/tests/a302_sealed.rs tools/description-pack/tests/a315_sealed.rs
{
  sha256sum Cargo.toml Cargo.lock
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
find tools/description-pack -type f ! -name '._*' ! -name '.DS_Store' ! -name 'a302_sealed.rs' ! -name 'a315_sealed.rs' | sort | xargs sha256sum > /evidence/pack-source.sha256

worst=0
if [ "$lock_exit" -ne 0 ]; then worst=$lock_exit; fi
if [ "$a302_exit" -ne 0 ]; then worst=$a302_exit; fi
if [ "$a315_exit" -ne 0 ]; then worst=$a315_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
if [ "$(cat /evidence/product-unchanged.exit)" -ne 0 ]; then worst=2; fi
echo "worst_phase_exit=$worst"
exit "$worst"
