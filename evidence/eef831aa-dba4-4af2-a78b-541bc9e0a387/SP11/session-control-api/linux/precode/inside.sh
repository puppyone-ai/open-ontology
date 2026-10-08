#!/bin/bash
# Pre-edit Linux arm64 Rust 1.94 smoke. session-control-api must be absent.
# No clippy and no PostgreSQL in this phase.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1

echo "=== identity ==="
rustc -vV
echo "rustc_vv_exit=$?"
cargo -V
uname -a
id

echo "=== absence ==="
if [ ! -f /src/tools/session-transition/src/reduce.rs ]; then
  echo "fixed archive missing the reducer" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ ! -f /src/tools/session-store/src/store.rs ]; then
  echo "fixed archive missing session-store" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ ! -f /src/tools/session-release/src/release.rs ]; then
  echo "fixed archive missing session-release" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
if [ -e /src/tools/session-control-api ]; then
  echo "session-control-api present before code" >&2
  printf '%s\n' 2 > /evidence/absence.exit
  exit 2
fi
if [ -f /src/rust-toolchain.toml ]; then
  echo "toolchain file must stay out of this archive" >&2
  printf '%s\n' 2 > /evidence/archive.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/absence.exit
printf '%s\n' 0 > /evidence/archive.exit

echo "=== rustc version gate ==="
if ! rustc -vV | grep -q '^release: 1.94.0$'; then
  echo "toolchain is not 1.94.0" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
if ! rustc -vV | grep -q '^host: aarch64-unknown-linux-gnu$'; then
  echo "host is not linux arm64" >&2
  printf '%s\n' 2 > /evidence/version.exit
  exit 2
fi
printf '%s\n' 0 > /evidence/version.exit

rm -rf /work/tools
mkdir -p /work
cp -R /src/tools /work/tools

hash_tree() {
  dir="$1"
  out="$2"
  (cd "$dir" && find Cargo.toml Cargo.lock CONTRACT.md src tests migrations -type f 2>/dev/null | sort | xargs sha256sum > "$out")
}
hash_tree /work/tools/session-transition /evidence/reducer-sha256.txt
hash_tree /work/tools/session-store /evidence/store-sha256.txt
hash_tree /work/tools/session-release /evidence/release-sha256.txt

run_one() {
  name="$1"
  cd "/work/tools/$name"
  echo "=== cargo fetch $name ==="
  set +e
  cargo fetch --locked > "/evidence/fetch-${name}.stdout" 2> "/evidence/fetch-${name}.stderr"
  fetch_exit=$?
  set -e
  printf '%s\n' "$fetch_exit" > "/evidence/fetch-${name}.exit"
  echo "PHASE fetch_${name}_exit=$fetch_exit"
  if [ "$fetch_exit" -ne 0 ]; then
    printf '%s\n' not_run > "/evidence/test-${name}.exit"
    exit "$fetch_exit"
  fi
  echo "=== cargo test $name lib locked offline ==="
  set +e
  cargo test --locked --offline --lib -- --test-threads=1 > "/evidence/test-${name}.stdout" 2> "/evidence/test-${name}.stderr"
  test_exit=$?
  set -e
  printf '%s\n' "$test_exit" > "/evidence/test-${name}.exit"
  echo "PHASE test_${name}_exit=$test_exit"
  if [ "$test_exit" -ne 0 ]; then
    exit "$test_exit"
  fi
}

run_one session-transition
run_one session-store
run_one session-release
if [ -e /work/tools/session-control-api ]; then
  echo "session-control-api appeared during smoke" >&2
  printf '%s\n' 2 > /evidence/absence.exit
  exit 2
fi
echo "=== smoke done ==="
exit 0
