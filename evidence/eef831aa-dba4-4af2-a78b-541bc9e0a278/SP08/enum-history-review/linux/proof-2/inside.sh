#!/bin/bash
# Pre-overlay smoke of the sealed 23 tests, then the independent overlay.
# Copy uses cp -R. The archive stays read-only. No lock generation. No Python.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
printf '%s\n' no > /evidence/cargo-update-invoked.txt

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
echo "cargo_v_exit=$?"
rustup show
uname -a
cat /etc/os-release
id
if ! rustc -vV | grep -q '^release: 1.94.0$'; then
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
if [ ! -f tools/enum-history/Cargo.lock ]; then
  echo "archive is missing the child Cargo.lock" >&2
  printf '%s\n' missing > /evidence/lock.exit
  exit 2
fi
printf '%s\n' present > /evidence/lock.exit
find tools/enum-history -type f ! -name '._*' | sort | xargs sha256sum > /evidence/source-sha256.txt
{
  sha256sum Cargo.lock tools/enum-history/Cargo.toml tools/enum-history/Cargo.lock tools/enum-history/CONTRACT.md
  find tools/enum-history/src tools/enum-history/tests -type f | sort | xargs sha256sum
} > /evidence/src-before.txt

python_present=0
if command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
  python_present=1
fi
printf '%s\n' "$python_present" > /evidence/python-present.flag
echo "python_present_flag=$python_present"

cmp_one() {
  if ! cmp -s "$1" "$2"; then
    echo "blob mismatch $1" >&2
    printf '%s\n' 2 > /evidence/blob-cmp.exit
    exit 2
  fi
}
while IFS= read -r rel; do
  cmp_one "tools/enum-history/$rel" "/blobs/$rel"
done < <(cd /blobs && find . -type f | sed 's|^\./||' | sort)
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -f tools/enum-history/tests/a278_handcomputed.rs ] || [ -f /src/tools/enum-history/tests/a278_handcomputed.rs ]; then
  echo "overlay present before smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit

echo "=== registry triples ==="
rustc --edition 2024 -D warnings /tools/lock_subset.rs -o /usr/local/bin/lock_subset
lock_compile=$?
printf '%s\n' "$lock_compile" > /evidence/lock-compile.exit
if [ "$lock_compile" -ne 0 ]; then
  echo "lock_subset did not compile" >&2
  exit "$lock_compile"
fi
set +e
/usr/local/bin/lock_subset tools/enum-history/Cargo.lock /src/Cargo.lock > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
lock_exit=$?
set -e
printf '%s\n' "$lock_exit" > /evidence/lock-subset.exit
echo "PHASE lock_subset_exit=$lock_exit"
awk '$1=="triple" { print $2, $3, $4, $5 }' /evidence/lock-subset.stdout > /evidence/lock-triples.txt
if ! cmp -s /evidence/lock-triples.txt /oracle/registry-triples.txt; then
  echo "registry triple list drifted" >&2
  printf '%s\n' 2 > /evidence/lock-compare.exit
  lock_exit=2
else
  printf '%s\n' 0 > /evidence/lock-compare.exit
fi
grep -q 'summary registry_triples=115 missing=0 mismatch=0 path_packages=2 direct_pins_present=true' /evidence/lock-subset.stdout || {
  echo "registry summary is not 115/0/0 path 2" >&2
  printf '%s\n' 2 > /evidence/lock-summary.exit
  lock_exit=2
}
if [ ! -f /evidence/lock-summary.exit ]; then
  printf '%s\n' 0 > /evidence/lock-summary.exit
fi

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path tools/enum-history/Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path tools/enum-history/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
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
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== pre-overlay cargo test ==="
set +e
cargo test --manifest-path tools/enum-history/Cargo.toml --locked --offline -- --test-threads=1 > /evidence/smoke-test.stdout 2> /evidence/smoke-test.stderr
smoke_exit=$?
set -e
printf '%s\n' "$smoke_exit" > /evidence/smoke-test.exit
echo "PHASE smoke_exit=$smoke_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/smoke-test.stdout > /evidence/smoke-count.txt
smoke_count=$(cat /evidence/smoke-count.txt)
echo "smoke_passed_count=$smoke_count"
missing_name=0
for name in \
  nineteen_twentieths_equals_lexical_0_95 \
  trailing_zero_keeps_its_denominator \
  one_hundred_over_one_hundred_five_is_above_0_95 \
  stdin_compiles_and_refuses \
  threshold_number_duplicate_and_malformed_exits \
  zero_rows_are_undefined_and_not_one \
  full_business_coverage_needs_no_review \
  boundary_includes_start_and_excludes_end \
  gap_keeps_both_rows_and_does_not_promote \
  dormant_future_overlap_and_historical_overlap_refuse \
  reversed_and_zero_intervals_refuse \
  retroactive_correction_preserves_history_and_declared_coverage \
  superseded_without_review_is_not_promoted \
  deprecated_with_review_keeps_status_and_historical_class \
  reserved_names_never_become_business_classes \
  inferred_approval_requires_core_review_fields \
  unknown_code_is_unexplained_and_exact_threshold_matches \
  active_undetermined_warns_even_when_unobserved \
  class_not_in_the_map_is_not_historical_business \
  duplicate_observation_and_overflow_refuse_without_a_report \
  observation_bound_precedes_core_parse \
  bad_date_and_threshold_and_core_yaml_refuse \
  canonical_enum_keeps_declared_coverage_separate
do
  if ! grep -q "$name" /evidence/smoke-test.stdout; then
    echo "smoke missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 23 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  if [ "$smoke_exit" -ne 0 ]; then
    exit "$smoke_exit"
  fi
  exit 1
fi
if grep -q 'a278_handcomputed' /evidence/smoke-test.stdout; then
  echo "overlay name appeared in the pre-overlay smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi

echo "=== overlay independent tests ==="
cp /overlay/a278_handcomputed.rs /work/tree/tools/enum-history/tests/a278_handcomputed.rs
if [ -f /src/tools/enum-history/tests/a278_handcomputed.rs ]; then
  echo "overlay leaked into the archive" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit
set +e
cargo test --manifest-path tools/enum-history/Cargo.toml --locked --offline --test a278_handcomputed -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  worst=$component_exit
  if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
  if [ "$lock_exit" -ne 0 ]; then worst=$lock_exit; fi
  exit "$worst"
fi

echo "=== cargo clippy ==="
set +e
cargo clippy --manifest-path tools/enum-history/Cargo.toml --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

{
  sha256sum Cargo.lock tools/enum-history/Cargo.toml tools/enum-history/Cargo.lock tools/enum-history/CONTRACT.md
  find tools/enum-history/src tools/enum-history/tests -type f ! -name 'a278_handcomputed.rs' | sort | xargs sha256sum
} > /evidence/src-after.txt
if cmp -s /evidence/src-before.txt /evidence/src-after.txt; then
  printf '%s\n' 0 > /evidence/product-unchanged.exit
else
  printf '%s\n' 2 > /evidence/product-unchanged.exit
  echo "product bytes changed inside the container" >&2
fi
cp tools/enum-history/Cargo.lock /evidence/Cargo.lock
sha256sum tools/enum-history/Cargo.lock | awk '{print $1}' > /evidence/child-lock.sha256
sha256sum Cargo.lock | awk '{print $1}' > /evidence/root-lock.sha256

worst=0
if [ "$lock_exit" -ne 0 ]; then worst=$lock_exit; fi
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
if [ "$(cat /evidence/product-unchanged.exit)" -ne 0 ]; then worst=2; fi
echo "worst_phase_exit=$worst"
exit "$worst"
