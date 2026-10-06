#!/bin/bash
# Rerun after linux/proof-2. That attempt is preserved.
# Author 16 description-review tests on the sealed archive, then description-pack.
# The archive stays read-only. No lock generation. No Python oracle.
# Origin shape: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a288/SP07/description-review-review/linux/proof-4/inside.sh
# content sha256 0f7ff79824026f1ee3b7a4a18eaa6ca27203f726caeeb4a9bbdfdc0e37f9ea62
# Change: copy tools/description-pack only after the 16-test smoke.
# Pre-overlay lock subset is the unmodified a288 tool (path count 4).
# Post-overlay lock subset requires path count 5, including description-pack.
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
if [ -d tools/description-pack ] || [ -d /src/tools/description-pack ]; then
  echo "description-pack present before smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
if [ ! -f tools/description-review/Cargo.lock ]; then
  echo "archive is missing the description-review Cargo.lock" >&2
  printf '%s\n' missing > /evidence/lock.exit
  exit 2
fi
printf '%s\n' present > /evidence/lock.exit
if [ ! -f crates/oo-builder/src/description_candidate.rs ]; then
  echo "archive is missing description_candidate.rs" >&2
  exit 2
fi
sha256sum crates/oo-builder/src/description_candidate.rs > /evidence/description-candidate.sha256
find tools/description-review -type f ! -name '._*' | sort | xargs sha256sum > /evidence/source-sha256.txt
{
  sha256sum Cargo.lock crates/oo-builder/src/description_candidate.rs
  sha256sum tools/description-review/Cargo.toml tools/description-review/Cargo.lock tools/description-review/CONTRACT.md
  find tools/description-review/src tools/description-review/tests -type f | sort | xargs sha256sum
} > /evidence/src-before.txt

python_present=0
if command -v python3 >/dev/null 2>&1 || command -v python >/dev/null 2>&1; then
  python_present=1
fi
printf '%s\n' "$python_present" > /evidence/python-present.flag
echo "python_present_flag=$python_present"

echo "=== registry triples before overlay ==="
set +e
rustc --edition 2024 -D warnings /tools/lock_subset_review.rs -o /usr/local/bin/lock_subset_review
lock_compile=$?
set -e
printf '%s\n' "$lock_compile" > /evidence/lock-compile.exit
if [ "$lock_compile" -ne 0 ]; then
  echo "review lock_subset did not compile" >&2
  exit "$lock_compile"
fi
set +e
/usr/local/bin/lock_subset_review tools/description-review/Cargo.lock /src/Cargo.lock > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
lock_exit=$?
set -e
printf '%s\n' "$lock_exit" > /evidence/lock-subset.exit
echo "PHASE lock_subset_exit=$lock_exit"
if ! grep -q 'summary registry_triples=117 missing=0 mismatch=0 path_packages=4 direct_pins_present=true' /evidence/lock-subset.stdout; then
  echo "registry summary is not 117/0/0 path 4" >&2
  printf '%s\n' 2 > /evidence/lock-summary.exit
  lock_exit=2
fi
if [ ! -f /evidence/lock-summary.exit ]; then
  printf '%s\n' 0 > /evidence/lock-summary.exit
fi

echo "=== cargo fetch description-review locked offline ==="
set +e
cargo fetch --manifest-path tools/description-review/Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch description-review locked online ==="
  set +e
  cargo fetch --manifest-path tools/description-review/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
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
cargo test --manifest-path tools/description-review/Cargo.toml --locked --offline -- --test-threads=1 > /evidence/smoke-test.stdout 2> /evidence/smoke-test.stderr
smoke_exit=$?
set -e
printf '%s\n' "$smoke_exit" > /evidence/smoke-test.exit
echo "PHASE smoke_exit=$smoke_exit"
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/smoke-test.stdout > /evidence/smoke-count.txt
smoke_count=$(cat /evidence/smoke-count.txt)
echo "smoke_passed_count=$smoke_count"
missing_name=0
for name in \
  field_literals_are_independent_of_the_library_helper \
  approve_one_dataset_and_reject_the_other \
  defer_and_a_missing_decision_leave_curated_text \
  cas_mismatch_does_not_write_and_a_quoted_literal_does \
  stale_schema_and_evidence_block_every_dataset \
  one_stale_anchor_does_not_block_the_other_dataset \
  a_missing_curated_file_is_not_invented \
  decision_schema_and_candidate_hash_block_one_dataset \
  provider_failure_does_not_copy_the_captured_body \
  wrong_outcome_hash_and_a_compiler_error_return_no_document \
  unknown_keys_do_not_echo_an_injected_token \
  exact_core_bounds_apply_and_a_longer_label_is_not_truncated \
  a_backtick_description_is_refused_without_a_partial_write \
  reviewer_length_is_a_command_error_and_256_is_kept \
  duplicate_unknown_and_identity_fail_closed \
  cli_apply_matches_the_library_and_usage_stays_on_stderr
do
  if ! grep -q "$name" /evidence/smoke-test.stdout; then
    echo "smoke missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 16 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  if [ "$smoke_exit" -ne 0 ]; then
    exit "$smoke_exit"
  fi
  exit 1
fi
if grep -q 'description-pack' /evidence/smoke-test.stdout; then
  echo "description-pack appeared in the pre-overlay smoke" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi

echo "=== overlay description-pack ==="
if [ ! -f /overlay/Cargo.toml ] || [ ! -f /blobs/Cargo.toml ]; then
  echo "overlay snapshot is incomplete" >&2
  printf '%s\n' 2 > /evidence/overlay-cmp.exit
  exit 2
fi
overlay_list=$(cd /overlay && find . -type f ! -name '._*' ! -name '.DS_Store' | sed 's|^\./||' | sort)
blob_list=$(cd /blobs && find . -type f ! -name '._*' ! -name '.DS_Store' | sed 's|^\./||' | sort)
if [ "$overlay_list" != "$blob_list" ]; then
  echo "overlay file list differs from the launch snapshot" >&2
  printf '%s\n' 2 > /evidence/overlay-cmp.exit
  exit 2
fi
while IFS= read -r rel; do
  if [ -z "$rel" ]; then
    continue
  fi
  if ! cmp -s "/overlay/$rel" "/blobs/$rel"; then
    echo "overlay blob mismatch $rel" >&2
    printf '%s\n' 2 > /evidence/overlay-cmp.exit
    exit 2
  fi
done <<EOF
$overlay_list
EOF
printf '%s\n' 0 > /evidence/overlay-cmp.exit
cp -R /overlay /work/tree/tools/description-pack
find /work/tree/tools/description-pack -name '._*' -delete
find /work/tree/tools/description-pack -name '.DS_Store' -delete
if [ -d /src/tools/description-pack ]; then
  echo "description-pack leaked into the archive" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit

echo "=== registry triples after overlay ==="
set +e
rustc --edition 2024 -D warnings /tools/lock_subset.rs -o /usr/local/bin/lock_subset
pack_lock_compile=$?
set -e
printf '%s\n' "$pack_lock_compile" > /evidence/pack-lock-compile.exit
if [ "$pack_lock_compile" -ne 0 ]; then
  echo "pack lock_subset did not compile" >&2
  exit "$pack_lock_compile"
fi
set +e
/usr/local/bin/lock_subset tools/description-pack/Cargo.lock /src/Cargo.lock > /evidence/pack-lock-subset.stdout 2> /evidence/pack-lock-subset.stderr
pack_lock_exit=$?
set -e
printf '%s\n' "$pack_lock_exit" > /evidence/pack-lock-subset.exit
echo "PHASE pack_lock_subset_exit=$pack_lock_exit"
if ! grep -q 'summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true' /evidence/pack-lock-subset.stdout; then
  echo "pack registry summary is not 117/0/0 path 5" >&2
  printf '%s\n' 2 > /evidence/pack-lock-summary.exit
  pack_lock_exit=2
else
  printf '%s\n' 0 > /evidence/pack-lock-summary.exit
fi

echo "=== cargo fetch description-pack locked offline ==="
set +e
cargo fetch --manifest-path tools/description-pack/Cargo.toml --locked --offline > /evidence/fetch-pack-offline.stdout 2> /evidence/fetch-pack-offline.stderr
pack_offline_exit=$?
set -e
printf '%s\n' "$pack_offline_exit" > /evidence/fetch-pack-offline.exit
echo "PHASE pack_offline_fetch_exit=$pack_offline_exit"
pack_fetch_exit=$pack_offline_exit
if [ "$pack_offline_exit" -ne 0 ]; then
  echo "=== cargo fetch description-pack locked online ==="
  set +e
  cargo fetch --manifest-path tools/description-pack/Cargo.toml --locked > /evidence/fetch-pack.stdout 2> /evidence/fetch-pack.stderr
  pack_fetch_exit=$?
  set -e
else
  cp /evidence/fetch-pack-offline.stdout /evidence/fetch-pack.stdout
  cp /evidence/fetch-pack-offline.stderr /evidence/fetch-pack.stderr
fi
printf '%s\n' "$pack_fetch_exit" > /evidence/fetch-pack.exit
echo "PHASE pack_fetch_exit=$pack_fetch_exit"
if [ "$pack_fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$pack_fetch_exit"
fi

echo "=== description-pack cargo test ==="
set +e
cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test pack -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
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
  if [ "$pack_lock_exit" -ne 0 ]; then worst=$pack_lock_exit; fi
  if [ "$lock_exit" -ne 0 ]; then worst=$lock_exit; fi
  exit "$worst"
fi

echo "=== cargo clippy description-pack ==="
set +e
cargo clippy --manifest-path tools/description-pack/Cargo.toml --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

{
  sha256sum Cargo.lock crates/oo-builder/src/description_candidate.rs
  sha256sum tools/description-review/Cargo.toml tools/description-review/Cargo.lock tools/description-review/CONTRACT.md
  find tools/description-review/src tools/description-review/tests -type f | sort | xargs sha256sum
} > /evidence/src-after.txt
if cmp -s /evidence/src-before.txt /evidence/src-after.txt; then
  printf '%s\n' 0 > /evidence/product-unchanged.exit
else
  printf '%s\n' 2 > /evidence/product-unchanged.exit
  echo "description-review bytes changed inside the container" >&2
fi
cp tools/description-pack/Cargo.lock /evidence/Cargo.lock
sha256sum tools/description-pack/Cargo.lock | awk '{print $1}' > /evidence/child-lock.sha256
sha256sum Cargo.lock | awk '{print $1}' > /evidence/root-lock.sha256
find tools/description-pack -type f ! -name '._*' ! -name '.DS_Store' | sort | xargs sha256sum > /evidence/pack-source.sha256

worst=0
if [ "$lock_exit" -ne 0 ]; then worst=$lock_exit; fi
if [ "$pack_lock_exit" -ne 0 ]; then worst=$pack_lock_exit; fi
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
if [ "$(cat /evidence/product-unchanged.exit)" -ne 0 ]; then worst=2; fi
echo "worst_phase_exit=$worst"
exit "$worst"
