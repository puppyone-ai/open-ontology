#!/bin/bash
# Smoke the description-pack sealed in the fixed archive, then overlay the
# occupied-review tree. The archive stays read-only. No lock generation.
# No Python oracle. No PostgreSQL.
# Origin shape: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a292/SP07/description-pack/linux/proof-3/inside.sh
# content follows a288 proof-4 inside.sh sha256 0f7ff79824026f1ee3b7a4a18eaa6ca27203f726caeeb4a9bbdfdc0e37f9ea62.
# Change: the archive already contains description-pack. Pre-overlay runs that
# crate's eight tests. Post-overlay runs the expanded pack tests, Clippy, and
# the a302 seven-test boundary oracle from a read-only mount.
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
if [ ! -f tools/description-pack/Cargo.toml ] || [ ! -f tools/description-pack/tests/pack.rs ]; then
  echo "archived description-pack is incomplete" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' present > /evidence/overlay-absent.exit
if [ ! -f tools/description-review/Cargo.lock ]; then
  echo "archive is missing the description-review Cargo.lock" >&2
  printf '%s\n' missing > /evidence/lock.exit
  exit 2
fi
printf '%s\n' present > /evidence/lock.exit
if [ ! -f crates/oo-builder/src/draft.rs ] || [ ! -f crates/oo-builder/src/captured_pg.rs ]; then
  echo "archive is missing the captured compiler" >&2
  exit 2
fi
sha256sum crates/oo-builder/src/draft.rs crates/oo-builder/src/captured_pg.rs > /evidence/description-candidate.sha256
find tools/description-review -type f ! -name '._*' | sort | xargs sha256sum > /evidence/source-sha256.txt
{
  sha256sum Cargo.lock crates/oo-builder/src/draft.rs crates/oo-builder/src/captured_pg.rs
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
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/boundary.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== pre-overlay cargo test ==="
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
  reject_and_defer_leave_the_original_seal \
  stale_schema_and_cas_refuse_without_a_document \
  review_hash_and_artifact_mismatch_refuse \
  incomplete_compiler_does_not_leak_candidate_text \
  occupied_dataset_review_is_unsupported \
  unclassified_allowlist_is_unknown_pii \
  cli_matches_the_library
do
  if ! grep -q "$name" /evidence/smoke-test.stdout; then
    echo "smoke missing $name" >&2
    missing_name=1
  fi
done
printf '%s\n' "$missing_name" > /evidence/smoke-names.exit
if [ "$smoke_exit" -ne 0 ] || [ "$smoke_count" -ne 8 ] || [ "$missing_name" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/boundary.exit
  printf '%s\n' not_run > /evidence/clippy-component.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  if [ "$smoke_exit" -ne 0 ]; then
    exit "$smoke_exit"
  fi
  exit 1
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
  printf '%s\n' "$overlay_list" > /evidence/overlay-list.txt
  printf '%s\n' "$blob_list" > /evidence/blob-list.txt
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
rm -rf /work/tree/tools/description-pack
cp -R /overlay /work/tree/tools/description-pack
find /work/tree/tools/description-pack -name '._*' -delete
find /work/tree/tools/description-pack -name '.DS_Store' -delete
if [ ! -f /src/tools/description-pack/src/apply.rs ]; then
  echo "description-pack missing from the read-only archive" >&2
  printf '%s\n' 2 > /evidence/overlay-present.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-present.exit

echo "=== registry triples after overlay ==="
set +e
/usr/local/bin/lock_subset tools/description-pack/Cargo.lock /src/Cargo.lock > /evidence/pack-lock-subset.stdout 2> /evidence/pack-lock-subset.stderr
pack_lock_exit=$?
set -e
printf '%s\n' "$pack_lock_exit" > /evidence/pack-lock-subset.exit
printf '%s\n' 0 > /evidence/pack-lock-compile.exit
echo "PHASE pack_lock_subset_exit=$pack_lock_exit"
if ! grep -q 'summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true' /evidence/pack-lock-subset.stdout; then
  echo "pack registry summary is not 117/0/0 path 5" >&2
  printf '%s\n' 2 > /evidence/pack-lock-summary.exit
  pack_lock_exit=2
else
  printf '%s\n' 0 > /evidence/pack-lock-summary.exit
fi

echo "=== cargo fetch description-pack locked offline after overlay ==="
set +e
cargo fetch --manifest-path tools/description-pack/Cargo.toml --locked --offline > /evidence/fetch-pack-offline.stdout 2> /evidence/fetch-pack-offline.stderr
pack_offline_exit=$?
set -e
printf '%s\n' "$pack_offline_exit" > /evidence/fetch-pack-offline.exit
echo "PHASE pack_offline_fetch_exit=$pack_offline_exit"
pack_fetch_exit=$pack_offline_exit
if [ "$pack_offline_exit" -ne 0 ]; then
  echo "=== cargo fetch description-pack locked online after overlay ==="
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
  printf '%s\n' not_run > /evidence/boundary.exit
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
awk '/^test result:/ { passed += $4 } END { print passed+0 }' /evidence/test.stdout > /evidence/test-count.txt

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  printf '%s\n' not_run > /evidence/boundary.exit
  exit "$component_exit"
fi

echo "=== cargo clippy description-pack ==="
set +e
cargo clippy --manifest-path tools/description-pack/Cargo.toml --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"

echo "=== a302 seven-test boundary ==="
if [ "$test_exit" -ne 0 ] || [ "$clippy_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/boundary.exit
  boundary_exit=0
else
  cp /boundary/a302_sealed.rs /work/tree/tools/description-pack/tests/a302_sealed.rs
  set +e
  cargo test --manifest-path tools/description-pack/Cargo.toml --locked --offline --test a302_sealed -- --test-threads=1 > /evidence/boundary.stdout 2> /evidence/boundary.stderr
  boundary_exit=$?
  set -e
  printf '%s\n' "$boundary_exit" > /evidence/boundary.exit
  echo "PHASE boundary_exit=$boundary_exit"
  rm -f /work/tree/tools/description-pack/tests/a302_sealed.rs
fi

{
  sha256sum Cargo.lock crates/oo-builder/src/draft.rs crates/oo-builder/src/captured_pg.rs
  sha256sum tools/description-review/Cargo.toml tools/description-review/Cargo.lock tools/description-review/CONTRACT.md
  find tools/description-review/src tools/description-review/tests -type f | sort | xargs sha256sum
} > /evidence/src-after.txt
if cmp -s /evidence/src-before.txt /evidence/src-after.txt; then
  printf '%s\n' 0 > /evidence/product-unchanged.exit
else
  printf '%s\n' 2 > /evidence/product-unchanged.exit
  echo "upstream bytes changed inside the container" >&2
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
if [ "$boundary_exit" -ne 0 ]; then worst=$boundary_exit; fi
if [ "$(cat /evidence/product-unchanged.exit)" -ne 0 ]; then worst=2; fi
echo "worst_phase_exit=$worst"
exit "$worst"
