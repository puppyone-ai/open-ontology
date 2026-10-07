#!/bin/bash
# Post-edit proof. /src is the fixed archive, mounted read-only.
# /overlay is the owned column-candidate files only. Locks are not regenerated.
# /work is a bind mount and is not removed.
set -u
set -e
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/smoke
export CARGO_BUILD_JOBS=1
export CARGO_TERM_COLOR=never
export TMPDIR=/cargo-home/tmp
mkdir -p /cargo-home/tmp /evidence/phases
printf '%s\n' no > /evidence/cargo-update-invoked.txt

record() {
  printf '%s\n' "$2" > "/evidence/phases/$1.exit"
  echo "PHASE $1=$2"
}

echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
uname -a | tee /evidence/uname.txt
id
rustc_exit=0
grep -q 'release: 1.94' /evidence/rustc-version.txt || rustc_exit=1
record toolchain "$rustc_exit"
if [ "$rustc_exit" -ne 0 ]; then
  exit "$rustc_exit"
fi

echo "=== copy archive and apply the owned overlay ==="
find /work -mindepth 1 -maxdepth 1 -exec rm -rf {} +
cp -a /src/. /work/
rm -f /work/rust-toolchain.toml
while IFS= read -r rel; do
  [ -n "$rel" ] || continue
  mkdir -p "/work/$(dirname "$rel")"
  cp "/overlay/$rel" "/work/$rel"
done < /evidence/overlay-files.txt
cd /work
overlay_exit=0
while read -r hash rel; do
  [ -n "$hash" ] || continue
  got=$(sha256sum "/work/$rel" | awk '{print $1}')
  if [ "$got" != "$hash" ]; then
    echo "overlay hash mismatch $rel" >&2
    overlay_exit=2
  fi
done < /evidence/overlay.sha256
if cmp -s /src/crates/oo-builder/src/description_candidate.rs /work/crates/oo-builder/src/description_candidate.rs; then
  echo "overlay did not replace description_candidate.rs" >&2
  overlay_exit=2
fi
if ! cmp -s /src/crates/oo-builder/src/bin/description-candidate.rs /work/crates/oo-builder/src/bin/description-candidate.rs; then
  echo "description-candidate bin differs from the fixed archive" >&2
  overlay_exit=2
fi
for lock in Cargo.lock tools/description-review/Cargo.lock tools/description-pack/Cargo.lock tools/description-provider/Cargo.lock; do
  if ! cmp -s "/src/$lock" "/work/$lock"; then
    echo "lock differs from the fixed archive: $lock" >&2
    overlay_exit=2
  fi
done
record overlay "$overlay_exit"
if [ "$overlay_exit" -ne 0 ]; then
  exit "$overlay_exit"
fi
sha256sum Cargo.lock > /evidence/root-lock-before.sha256

run_phase() {
  local name="$1"
  shift
  set +e
  "$@" > "/evidence/${name}.log" 2>&1
  local status=$?
  set -e
  record "$name" "$status"
  if [ "$status" -ne 0 ]; then
    exit "$status"
  fi
}

echo "=== oo-builder description candidate API and CLI ==="
run_phase test-candidate cargo test --locked -p oo-builder --test description_candidate --test description_candidate_cli -- --test-threads=1

echo "=== oo-builder description_candidate lib tests ==="
run_phase test-lib cargo test --locked -p oo-builder --lib description_candidate -- --test-threads=1
if ! grep -q 'prompt_version_is_the_sealed_constant' /evidence/test-lib.log; then
  echo "lib filter missed the sealed prompt test" >&2
  record test-lib-filter 1
  exit 1
fi
record test-lib-filter 0

echo "=== description-review ==="
run_phase test-review cargo test --locked --manifest-path tools/description-review/Cargo.toml -- --test-threads=1

echo "=== description-pack ==="
run_phase test-pack cargo test --locked --manifest-path tools/description-pack/Cargo.toml -- --test-threads=1

if [ ! -x /target/smoke/debug/description-candidate ]; then
  echo "description-candidate binary is missing" >&2
  record test-candidate-bin 1
  exit 1
fi
export OO_A263_DESCRIPTION_CANDIDATE=/target/smoke/debug/description-candidate
printf '%s\n' "$OO_A263_DESCRIPTION_CANDIDATE" > /evidence/description-candidate-bin.txt
record test-candidate-bin 0

echo "=== description-provider ==="
run_phase test-provider cargo test --locked --manifest-path tools/description-provider/Cargo.toml -- --test-threads=1

echo "=== clippy component ==="
set +e
if rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  component_exit=0
  printf '%s\n' already_installed > /evidence/clippy-component.log
else
  rustup component add clippy --toolchain 1.94.0 > /evidence/clippy-component.log 2>&1
  component_exit=$?
fi
set -e
record clippy-component "$component_exit"
if [ "$component_exit" -ne 0 ]; then
  exit "$component_exit"
fi

echo "=== clippy oo-builder description candidate ==="
run_phase clippy-builder cargo clippy --locked --offline -p oo-builder --lib --bin description-candidate --test description_candidate --test description_candidate_cli -- -D warnings

echo "=== clippy description-review ==="
run_phase clippy-review cargo clippy --locked --offline --manifest-path tools/description-review/Cargo.toml --all-targets -- -D warnings

echo "=== clippy description-pack ==="
run_phase clippy-pack cargo clippy --locked --offline --manifest-path tools/description-pack/Cargo.toml --all-targets -- -D warnings

echo "=== clippy description-provider ==="
run_phase clippy-provider cargo clippy --locked --offline --manifest-path tools/description-provider/Cargo.toml --all-targets -- -D warnings

echo "=== description-pack lock subset ==="
set +e
rustc --edition 2024 -D warnings /evidence/lock_subset.rs -o /usr/local/bin/lock_subset > /evidence/lock-compile.log 2>&1
lock_compile=$?
set -e
record lock-compile "$lock_compile"
if [ "$lock_compile" -ne 0 ]; then
  exit "$lock_compile"
fi
set +e
/usr/local/bin/lock_subset tools/description-pack/Cargo.lock /work/Cargo.lock > /evidence/lock-subset.stdout 2> /evidence/lock-subset.stderr
lock_exit=$?
set -e
record lock-subset "$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  exit "$lock_exit"
fi
if ! grep -q 'summary registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true' /evidence/lock-subset.stdout; then
  echo "pack lock summary drifted" >&2
  record lock-summary 2
  exit 2
fi
record lock-summary 0

sha256sum Cargo.lock > /evidence/root-lock-after.sha256
if ! cmp -s /evidence/root-lock-before.sha256 /evidence/root-lock-after.sha256; then
  echo "root lock bytes changed" >&2
  record root-lock 1
  exit 1
fi
for lock in tools/description-review/Cargo.lock tools/description-pack/Cargo.lock tools/description-provider/Cargo.lock; do
  sha256sum "$lock" >> /evidence/child-locks.sha256
  if ! cmp -s "/src/$lock" "/work/$lock"; then
    echo "child lock bytes changed: $lock" >&2
    record root-lock 1
    exit 1
  fi
done
record root-lock 0
echo "PROOF_OK"
