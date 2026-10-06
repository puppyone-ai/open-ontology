#!/bin/bash
# Pre-overlay smoke of the sealed 21 tests, then the independent overlay.
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
mkdir -p /work/tree/examples/schema
cp /src/examples/schema/calendar.yaml /work/tree/examples/schema/calendar.yaml
find /work/tree -name '._*' -delete
find /work/tree -name '.DS_Store' -delete
cd /work/tree
if [ ! -f examples/schema/calendar.yaml ]; then
  echo "work tree is missing examples/schema/calendar.yaml" >&2
  printf '%s\n' missing > /evidence/example-calendar.exit
  exit 2
fi
sha256sum examples/schema/calendar.yaml > /evidence/example-calendar.before.txt
printf '%s\n' present > /evidence/example-calendar.exit
if [ ! -f tools/business-calendar/Cargo.lock ]; then
  echo "archive is missing the child Cargo.lock" >&2
  printf '%s\n' missing > /evidence/lock.exit
  exit 2
fi
printf '%s\n' present > /evidence/lock.exit
find tools/business-calendar -type f ! -name '._*' | sort | xargs sha256sum > /evidence/source-sha256.txt
{
  sha256sum Cargo.lock tools/business-calendar/Cargo.toml tools/business-calendar/Cargo.lock tools/business-calendar/CONTRACT.md
  find tools/business-calendar/src tools/business-calendar/tests -type f | sort | xargs sha256sum
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
  cmp_one "tools/business-calendar/$rel" "/blobs/$rel"
done < <(cd /blobs && find . -type f | sed 's|^\./||' | sort)
printf '%s\n' 0 > /evidence/blob-cmp.exit
if [ -f tools/business-calendar/tests/a283_handcomputed.rs ] || [ -f /src/tools/business-calendar/tests/a283_handcomputed.rs ]; then
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
/usr/local/bin/lock_subset tools/business-calendar/Cargo.lock /src/Cargo.lock > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
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
cargo fetch --manifest-path tools/business-calendar/Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path tools/business-calendar/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
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
cargo test --manifest-path tools/business-calendar/Cargo.toml --locked --offline -- --test-threads=1 > /evidence/smoke-test.stdout 2> /evidence/smoke-test.stderr
smoke_exit=$?
set -e
printf '%s\n' "$smoke_exit" > /evidence/smoke-test.exit
echo "PHASE smoke_exit=$smoke_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/smoke-test.stdout > /evidence/smoke-count.txt
smoke_count=$(cat /evidence/smoke-count.txt)
echo "smoke_passed_count=$smoke_count"
missing_name=0
for name in \
  unix_day_counts \
  roundtrip_and_edges \
  fiscal_quarters_are_not_calendar_quarters \
  exit_table \
  handwritten_instants \
  month_to_date_clamp \
  day_count_is_not_a_date_and_max_is_later \
  known_answers \
  resolved_stdout_is_exact \
  help_and_usage \
  malformed_contract_and_second_value \
  bounds_and_redaction \
  closure_and_expression_exits \
  example_calendar_shanghai_april \
  fiscal_february_is_not_the_calendar_quarter \
  fiscal_april_and_year_boundary_and_leap_day \
  month_to_date_clamp_and_day_arithmetic \
  zone_date_split_and_dst_lengths \
  missing_and_ambiguous_midnights_stay_unshifted \
  refusals_keep_provenance_and_do_not_substitute \
  source_does_not_read_a_clock
do
  if ! grep -q "$name" /evidence/smoke-test.stdout; then
    echo "smoke missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 21 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  if [ "$smoke_exit" -ne 0 ]; then
    exit "$smoke_exit"
  fi
  exit 1
fi
if grep -q 'a283_handcomputed' /evidence/smoke-test.stdout; then
  echo "overlay name appeared in the pre-overlay smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi

echo "=== overlay independent tests ==="
cp /overlay/a283_handcomputed.rs /work/tree/tools/business-calendar/tests/a283_handcomputed.rs
if [ -f /src/tools/business-calendar/tests/a283_handcomputed.rs ]; then
  echo "overlay leaked into the archive" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit
set +e
cargo test --manifest-path tools/business-calendar/Cargo.toml --locked --offline --test a283_handcomputed -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
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
cargo clippy --manifest-path tools/business-calendar/Cargo.toml --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

{
  sha256sum Cargo.lock tools/business-calendar/Cargo.toml tools/business-calendar/Cargo.lock tools/business-calendar/CONTRACT.md
  find tools/business-calendar/src tools/business-calendar/tests -type f ! -name 'a283_handcomputed.rs' | sort | xargs sha256sum
} > /evidence/src-after.txt
if cmp -s /evidence/src-before.txt /evidence/src-after.txt; then
  printf '%s\n' 0 > /evidence/product-unchanged.exit
else
  printf '%s\n' 2 > /evidence/product-unchanged.exit
  echo "product bytes changed inside the container" >&2
fi
sha256sum examples/schema/calendar.yaml > /evidence/example-calendar.after.txt
if cmp -s /evidence/example-calendar.before.txt /evidence/example-calendar.after.txt; then
  printf '%s\n' 0 > /evidence/example-calendar-unchanged.exit
else
  printf '%s\n' 2 > /evidence/example-calendar-unchanged.exit
  echo "example calendar.yaml changed inside the container" >&2
fi
cp tools/business-calendar/Cargo.lock /evidence/Cargo.lock
sha256sum tools/business-calendar/Cargo.lock | awk '{print $1}' > /evidence/child-lock.sha256
sha256sum Cargo.lock | awk '{print $1}' > /evidence/root-lock.sha256

worst=0
if [ "$lock_exit" -ne 0 ]; then worst=$lock_exit; fi
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
if [ "$(cat /evidence/product-unchanged.exit)" -ne 0 ]; then worst=2; fi
if [ "$(cat /evidence/example-calendar-unchanged.exit)" -ne 0 ]; then worst=2; fi
echo "worst_phase_exit=$worst"
exit "$worst"
