#!/bin/bash
# Post-edit boundary plus the evidence probe. Root lock is read-only.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
export CARGO_NET_OFFLINE=true

phase() {
  printf '%s\n' "$2" > "/evidence/$1.exit"
  echo "PHASE $1=$2"
}

cd /src
echo "=== identity ==="
rustc -vV | tee /evidence/rustc-version.txt
cargo -V | tee /evidence/cargo-version.txt
rustup show | tee /evidence/rustup-show.txt
if ! grep -q 'release: 1.94' /evidence/rustc-version.txt; then
  echo "compiler is not rustc 1.94" >&2
  phase check 2
  exit 2
fi

echo "=== root manifests ==="
sha256sum Cargo.toml Cargo.lock > /evidence/manifest-sha256.txt
cmp -s Cargo.lock /evidence/expected-lock.bytes || {
  echo "mounted root lock does not match the expected frozen bytes" >&2
  phase lock 2
  exit 2
}
if [ -f /src/evidence/eef831aa-dba4-4af2-a78b-541bc9e0a313/SP00/gateway-dependencies/probe/Cargo.toml ]; then
  echo "probe is inside the root archive" >&2
  exit 2
fi

echo "=== root metadata locked no-deps ==="
set +e
cargo metadata --manifest-path Cargo.toml --locked --format-version 1 --no-deps > /evidence/metadata.stdout 2> /evidence/metadata.stderr
metadata_exit=$?
set -e
phase metadata "$metadata_exit"
if [ "$metadata_exit" -ne 0 ]; then
  phase check not_run
  exit "$metadata_exit"
fi

echo "=== root check oo-arch-check offline ==="
set +e
cargo check --manifest-path Cargo.toml --locked --offline -p oo-arch-check > /evidence/check.stdout 2> /evidence/check.stderr
check_exit=$?
set -e
phase check "$check_exit"
if [ "$check_exit" -ne 0 ]; then
  exit "$check_exit"
fi
sha256sum Cargo.toml Cargo.lock > /evidence/manifest-sha256-after-check.txt
if ! cmp -s /evidence/manifest-sha256.txt /evidence/manifest-sha256-after-check.txt; then
  echo "root manifest hash changed during boundary check" >&2
  phase lock 2
  exit 2
fi

echo "=== compile oracles ==="
set +e
rustc --edition 2024 -D warnings -o /target/lockdiff /tools/lockdiff.rs > /evidence/lockdiff-build.stdout 2> /evidence/lockdiff-build.stderr
lockdiff_build_exit=$?
rustc --edition 2021 -D warnings -o /target/sha256_manifest /tools/sha256_manifest.rs > /evidence/sha256-build.stdout 2> /evidence/sha256-build.stderr
sha256_build_exit=$?
set -e
phase lockdiff_build "$lockdiff_build_exit"
phase sha256_build "$sha256_build_exit"
if [ "$lockdiff_build_exit" -ne 0 ] || [ "$sha256_build_exit" -ne 0 ]; then
  exit 1
fi
set +e
/target/lockdiff selfcheck > /evidence/lockdiff-selfcheck.stdout 2> /evidence/lockdiff-selfcheck.stderr
self_exit=$?
set -e
phase lockdiff_selfcheck "$self_exit"
if [ "$self_exit" -ne 0 ]; then
  exit "$self_exit"
fi

echo "=== probe fetch ==="
unset CARGO_NET_OFFLINE
set +e
cargo fetch --manifest-path /probe/Cargo.toml > /evidence/fetch.stdout 2> /evidence/fetch.stderr
fetch_exit=$?
set -e
export CARGO_NET_OFFLINE=true
phase fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  if [ -f /probe/Cargo.lock ]; then
    cp /probe/Cargo.lock /evidence/probe-lock-rejected.lock
  fi
  exit "$fetch_exit"
fi
cp /probe/Cargo.lock /evidence/probe-Cargo.lock

echo "=== lockdiff subset ==="
set +e
/target/lockdiff subset /src/Cargo.lock /probe/Cargo.lock /upstream/new-crate-pins.txt 2026-10-06T12:47:28Z > /evidence/lockdiff-subset.stdout 2> /evidence/lockdiff-subset.stderr
subset_exit=$?
set -e
phase lockdiff_subset "$subset_exit"
if [ "$subset_exit" -ne 0 ]; then
  cp /probe/Cargo.lock /evidence/probe-lock-rejected.lock
  exit "$subset_exit"
fi

echo "=== lockdiff exact root against frozen bytes ==="
set +e
/target/lockdiff exact /evidence/expected-lock.bytes /src/Cargo.lock > /evidence/lockdiff-root.stdout 2> /evidence/lockdiff-root.stderr
root_diff_exit=$?
set -e
phase lockdiff_root "$root_diff_exit"
if [ "$root_diff_exit" -ne 0 ]; then
  exit "$root_diff_exit"
fi

echo "=== crate checksums and license bytes ==="
mkdir -p /evidence/licenses
while read -r name version expect; do
  case "$name" in
    ''|\#*) continue ;;
  esac
  stage=/target/crate-stage
  rm -rf "$stage"
  mkdir -p "$stage"
  found=$(find /cargo-home/registry/cache -type f -name "${name}-${version}.crate" | sort)
  count=$(printf '%s\n' "$found" | sed '/^$/d' | wc -l | tr -d ' ')
  if [ "$count" -ne 1 ]; then
    echo "expected one ${name}-${version}.crate, found ${count}" >&2
    printf '%s\n' "$found" > "/evidence/licenses/${name}-${version}.find"
    phase licenses 1
    exit 1
  fi
  cp "$found" "$stage/${name}-${version}.crate"
  /target/sha256_manifest "$stage" "/evidence/licenses/${name}-${version}.sha256" > "/evidence/licenses/${name}-${version}.sha256.log"
  got=$(awk 'NR==1 { print $1 }' "/evidence/licenses/${name}-${version}.sha256")
  if [ "$got" != "$expect" ]; then
    echo "${name} ${version} checksum ${got} != ${expect}" >&2
    phase licenses 1
    exit 1
  fi
  dest="/evidence/licenses/${name}-${version}"
  rm -rf "$dest"
  mkdir -p "$dest"
  tar -xzf "$stage/${name}-${version}.crate" -C "$dest"
  echo "verified ${name} ${version} ${got}" >> /evidence/licenses/verified.txt
done < /upstream/license-crates.txt
phase licenses 0

echo "=== clippy component ==="
unset CARGO_NET_OFFLINE
if ! rustup component list --installed | grep -q '^clippy'; then
  rustup component add clippy > /evidence/clippy-install.stdout 2> /evidence/clippy-install.stderr
  phase clippy_install $?
else
  printf '%s\n' already_installed > /evidence/clippy-install.stdout
  phase clippy_install 0
fi
export CARGO_NET_OFFLINE=true

echo "=== probe test ==="
set +e
cargo test --manifest-path /probe/Cargo.toml --locked --offline -- --test-threads=1 > /evidence/test.stdout 2> /evidence/test.stderr
test_exit=$?
set -e
phase test "$test_exit"
if [ "$test_exit" -ne 0 ]; then
  exit "$test_exit"
fi

echo "=== probe clippy ==="
set +e
cargo clippy --manifest-path /probe/Cargo.toml --locked --offline --all-targets -- -D warnings > /evidence/clippy.stdout 2> /evidence/clippy.stderr
clippy_exit=$?
set -e
phase clippy "$clippy_exit"
if [ "$clippy_exit" -ne 0 ]; then
  exit "$clippy_exit"
fi

sha256sum /src/Cargo.toml /src/Cargo.lock /probe/Cargo.toml /probe/Cargo.lock > /evidence/manifest-sha256-after.txt
if ! cmp -s /src/Cargo.lock /evidence/expected-lock.bytes; then
  echo "root lock bytes changed" >&2
  phase lock 2
  exit 2
fi
phase lock 0
/target/sha256_manifest /evidence /target/SHA256SUMS > /target/sha256-inventory.log
cp /target/SHA256SUMS /evidence/SHA256SUMS
cp /target/sha256-inventory.log /evidence/sha256-inventory.log
echo "inside_ok=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
exit 0
