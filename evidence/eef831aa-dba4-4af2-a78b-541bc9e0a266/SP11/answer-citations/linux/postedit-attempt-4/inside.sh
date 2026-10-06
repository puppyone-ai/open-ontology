#!/bin/bash
# Post-edit focused fetch, test, and Clippy on the overlay copy.
# Linux arm64 Rust 1.94. The source mount is read-only. No cargo update.
# No apt. No PostgreSQL. Fetch uses the bridge network. Test and Clippy are offline.
set -u
echo "oo_task=sp11-answer-citations-postedit"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a266"
echo "oo_fixed=567d621e2e52b7e0011bdb025dc9337e92ecc5a6"
echo "oo_platform=linux/arm64"
echo "phase=postedit"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/postedit-attempt-4/phases /cargo-home/bin
cd /src
printf '%s\n' "not-invoked" > /evidence/linux/postedit-attempt-4/cargo-update-invoked.txt
MANIFEST=tools/answer-citations/Cargo.toml

record() {
  mkdir -p /evidence/linux/postedit-attempt-4/phases
  printf '%s\n' "$2" > "/evidence/linux/postedit-attempt-4/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
}

fail() {
  record "$1" "$2"
  printf '%s\n' "$2" > /evidence/linux/postedit-attempt-4/postedit.exit
  exit "$2"
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/postedit-attempt-4/rustc-version.txt
rustc -Vv > /evidence/linux/postedit-attempt-4/rustc-verbose.txt
cargo -V | tee /evidence/linux/postedit-attempt-4/cargo-version.txt
rustup show > /evidence/linux/postedit-attempt-4/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; fail toolchain 2 ;;
esac
record toolchain 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  record sha256-tool $?
else
  record sha256-tool 0
fi
export PATH="/cargo-home/bin:${PATH}"
sha=$(cat /src/.fixed-base-sha)
printf '%s\n' "$sha" > /evidence/linux/postedit-attempt-4/fixed-base.txt
if [ "$sha" != "567d621e2e52b7e0011bdb025dc9337e92ecc5a6" ]; then
  echo "archive sha mismatch" >&2
  fail source 2
fi
if [ ! -f "$MANIFEST" ] || [ ! -f tools/answer-citations/src/lib.rs ]; then
  echo "answer-citations overlay is absent" >&2
  fail source 2
fi
sha256 dir /evidence/linux/postedit-attempt-4/package-sha256.txt /src/tools/answer-citations
hash_exit=$?
record package-hash "$hash_exit"
if [ "$hash_exit" -ne 0 ]; then
  fail package-hash "$hash_exit"
fi
record source 0

echo "=== rustup component add clippy ==="
set +e
rustup component add clippy --toolchain 1.94.0 > /evidence/linux/postedit-attempt-4/component.stdout 2> /evidence/linux/postedit-attempt-4/component.stderr
component_exit=$?
set +e
printf '%s\n' "$component_exit" > /evidence/linux/postedit-attempt-4/component.exit
echo "PHASE component_exit=$component_exit"
if [ "$component_exit" -ne 0 ]; then
  fail component "$component_exit"
fi
record component 0

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path "$MANIFEST" --locked > /evidence/linux/postedit-attempt-4/fetch.stdout 2> /evidence/linux/postedit-attempt-4/fetch.stderr
fetch_exit=$?
set +e
printf '%s\n' "$fetch_exit" > /evidence/linux/postedit-attempt-4/fetch.exit
echo "PHASE fetch_exit=$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  fail fetch "$fetch_exit"
fi
record fetch 0

echo "=== cargo test --locked --offline ==="
set +e
cargo test --manifest-path "$MANIFEST" --locked --offline --no-fail-fast -- --test-threads=1 > /evidence/linux/postedit-attempt-4/test.stdout 2> /evidence/linux/postedit-attempt-4/test.stderr
test_exit=$?
set +e
printf '%s\n' "$test_exit" > /evidence/linux/postedit-attempt-4/test.exit
echo "PHASE test_exit=$test_exit"
record test "$test_exit"

echo "=== cargo clippy --locked --offline ==="
set +e
cargo clippy --manifest-path "$MANIFEST" --locked --offline --all-targets -- -D warnings > /evidence/linux/postedit-attempt-4/clippy.stdout 2> /evidence/linux/postedit-attempt-4/clippy.stderr
clippy_exit=$?
set +e
printf '%s\n' "$clippy_exit" > /evidence/linux/postedit-attempt-4/clippy.exit
echo "PHASE clippy_exit=$clippy_exit"
record clippy "$clippy_exit"

if [ "$test_exit" -ne 0 ]; then
  printf '%s\n' "$test_exit" > /evidence/linux/postedit-attempt-4/postedit.exit
  exit "$test_exit"
fi
if [ "$clippy_exit" -ne 0 ]; then
  printf '%s\n' "$clippy_exit" > /evidence/linux/postedit-attempt-4/postedit.exit
  exit "$clippy_exit"
fi
printf '%s\n' 0 > /evidence/linux/postedit-attempt-4/postedit.exit
echo "postedit_done"
