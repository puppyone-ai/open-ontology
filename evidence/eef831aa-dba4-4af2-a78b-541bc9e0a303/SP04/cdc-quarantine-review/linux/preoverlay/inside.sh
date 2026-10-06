#!/bin/bash
# Pre-overlay Linux arm64 Rust 1.94 run of the author cdc-quarantine tests.
# The archive is 25dfc902. This script must not see the review overlay.
set -u
export PATH=/usr/local/cargo/bin:/usr/local/bin:$PATH
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
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
if ! rustc -vV | grep -q 'release: 1.94.0'; then
  echo "rustc is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

echo "=== source pins ==="
files="
CONTRACT.md
Cargo.lock
Cargo.toml
src/jsonline.rs
src/lib.rs
src/main.rs
src/model.rs
src/pathutil.rs
src/sha256.rs
src/store.rs
tests/common/mod.rs
tests/common/oracle.rs
tests/proof.rs
"
blob_ok=0
for rel in $files; do
  if ! cmp -s "tools/cdc-quarantine/$rel" "/blobs/tools/cdc-quarantine/$rel"; then
    echo "blob mismatch $rel" >&2
    blob_ok=2
  fi
done
printf '%s\n' "$blob_ok" > /evidence/blob-cmp.exit
if [ "$blob_ok" -ne 0 ]; then
  exit 2
fi
if [ -f tools/cdc-quarantine/tests/review_proof.rs ]; then
  echo "overlay present during preoverlay" >&2
  printf '%s\n' 2 > /evidence/overlay-absent.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/overlay-absent.exit
find tools/cdc-quarantine -type l -print > /evidence/symlinks.txt
if [ -s /evidence/symlinks.txt ]; then
  echo "symlink in quarantine tree" >&2
  exit 2
fi

echo "=== lock subset ==="
rustc --edition 2024 -D warnings /tools/lock_subset.rs -o /usr/local/bin/lock_subset
set +e
/usr/local/bin/lock_subset /src/Cargo.lock /src/tools/cdc-quarantine/Cargo.lock > /evidence/lock-subset.txt 2> /evidence/lock-subset.stderr
lock_exit=$?
set -e
printf '%s\n' "$lock_exit" > /evidence/lock-subset.exit
echo "PHASE lock_subset_exit=$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$lock_exit"
fi

echo "=== cargo fetch locked offline ==="
set +e
cargo fetch --manifest-path tools/cdc-quarantine/Cargo.toml --locked --offline > /evidence/fetch-offline.stdout 2> /evidence/fetch-offline.stderr
offline_fetch_exit=$?
set -e
printf '%s\n' "$offline_fetch_exit" > /evidence/fetch-offline.exit
echo "PHASE offline_fetch_exit=$offline_fetch_exit"
fetch_exit=$offline_fetch_exit
if [ "$offline_fetch_exit" -ne 0 ]; then
  echo "=== cargo fetch locked online ==="
  set +e
  cargo fetch --manifest-path tools/cdc-quarantine/Cargo.toml --locked > /evidence/fetch.stdout 2> /evidence/fetch.stderr
  fetch_exit=$?
  set -e
else
  cp /evidence/fetch-offline.stdout /evidence/fetch.stdout
  cp /evidence/fetch-offline.stderr /evidence/fetch.stderr
fi
printf '%s\n' "$fetch_exit" > /evidence/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/test.exit
  printf '%s\n' not_run > /evidence/clippy.exit
  exit "$fetch_exit"
fi

echo "=== cargo test author cdc-quarantine ==="
set +e
cargo test --manifest-path tools/cdc-quarantine/Cargo.toml --locked --offline --all-targets -- --test-threads=1 \
  > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
printf '%s\n' "$test_exit" > /evidence/test.exit
echo "PHASE test_exit=$test_exit"
awk '
  /^test result:/ {
    n++
    pass += $4
    fail += $6
    if ($3 != "ok.") bad++
  }
  END { printf "lines=%d passed=%d failed=%d bad=%d\n", n+0, pass+0, fail+0, bad+0 }
' /evidence/test.stdout > /evidence/test-count.txt
cat /evidence/test-count.txt
if ! grep -q 'passed=15 failed=0 bad=0' /evidence/test-count.txt; then
  echo "author test count is not 15 passed" >&2
  printf '%s\n' 2 > /evidence/count.exit
  count_exit=2
else
  printf '%s\n' 0 > /evidence/count.exit
  count_exit=0
fi

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.stdout 2> /evidence/clippy-component.stderr
component_exit=$?
set -e
printf '%s\n' "$component_exit" > /evidence/clippy-component.exit
echo "PHASE clippy_component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  printf '%s\n' not_run > /evidence/clippy.exit
  if [ "$test_exit" -ne 0 ]; then
    exit "$test_exit"
  fi
  if [ "$count_exit" -ne 0 ]; then
    exit "$count_exit"
  fi
  exit "$component_exit"
fi

echo "=== cargo clippy cdc-quarantine ==="
set +e
cargo clippy --manifest-path tools/cdc-quarantine/Cargo.toml --locked --offline --all-targets -- -D warnings \
  > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
printf '%s\n' "$clippy_exit" > /evidence/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
worst=0
if [ "$test_exit" -ne 0 ]; then worst=$test_exit; fi
if [ "$count_exit" -ne 0 ]; then worst=$count_exit; fi
if [ "$clippy_exit" -ne 0 ]; then worst=$clippy_exit; fi
echo "worst_phase_exit=$worst"
exit "$worst"
