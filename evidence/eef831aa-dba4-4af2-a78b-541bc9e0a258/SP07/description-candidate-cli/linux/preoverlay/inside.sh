#!/bin/bash
# Pre-edit focused smoke of the fixed description-candidate library.
# Linux arm64 Rust 1.94. No cargo update. No apt. No PostgreSQL. No model.
# Origin inside: a253 commit 268e46972458fe53dd54412ab277b2cd09552a82
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/linux/preoverlay/inside.sh
#   blob 4aecdc35b11e08e8c9a010f6ccf0fc6c4cefa5d2
# Changes: fixed fbc10b04, library must be present, CLI files must be absent,
# smoke the description_candidate tests instead of captured_pg.
set -u
echo "oo_task=sp07-description-candidate-cli-preoverlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a258"
echo "oo_fixed=fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a"
echo "oo_platform=linux/arm64"
echo "phase=preoverlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/preoverlay/phases /cargo-home/bin /cargo-home/work
rm -rf /cargo-home/work/src
mkdir -p /cargo-home/work/src
cp -a /src/. /cargo-home/work/src/
cd /cargo-home/work/src
if [ -f rust-toolchain.toml ]; then
  mv rust-toolchain.toml rust-toolchain.toml.aside
fi
printf '%s\n' "moved-aside" > /evidence/linux/preoverlay/toolchain-aside.txt

record() {
  mkdir -p /evidence/linux/preoverlay/phases
  printf '%s\n' "$2" > "/evidence/linux/preoverlay/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/preoverlay/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/preoverlay/rustc-version.txt
rustc -Vv > /evidence/linux/preoverlay/rustc-verbose.txt
cargo -V | tee /evidence/linux/preoverlay/cargo-version.txt
rustup show > /evidence/linux/preoverlay/rustup-show.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
if ! grep -q '1.94.0' /evidence/linux/preoverlay/rustup-show.txt; then
  echo "rustup show did not report 1.94.0" >&2
  record toolchain 2
fi
record toolchain 0

rustc --edition 2021 -D warnings /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"
/cargo-home/bin/sha256 file /tools/sha256.rs > /evidence/linux/preoverlay/sha256-tool.txt

if [ ! -f crates/oo-builder/src/description_candidate.rs ]; then
  echo "fixed archive is missing the description candidate library" >&2
  record source 2
fi
if [ ! -f contracts/builder/description-candidate-v1.md ]; then
  echo "fixed archive is missing the description candidate contract" >&2
  record source 2
fi
if [ -e crates/oo-builder/src/bin/description-candidate.rs ]; then
  echo "description-candidate CLI is already on the pre-edit archive" >&2
  record source 2
fi
if [ -e crates/oo-builder/tests/description_candidate_cli.rs ]; then
  echo "description-candidate CLI test is already on the pre-edit archive" >&2
  record source 2
fi
if [ -e contracts/builder/description-candidate-cli-v1.md ]; then
  echo "description-candidate CLI contract is already on the pre-edit archive" >&2
  record source 2
fi
printf '%s\n' "cli_overlay_absent" > /evidence/linux/preoverlay/overlay-absent.txt
record source 0

/cargo-home/bin/sha256 file \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/description_candidate.rs \
  crates/oo-builder/Cargo.toml \
  contracts/builder/description-candidate-v1.md \
  Cargo.toml \
  Cargo.lock \
  > /evidence/linux/preoverlay/owned-sha256.txt
record owned-hash $?
printf '%s\n' "no" > /evidence/linux/preoverlay/cargo-update-invoked.txt

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/preoverlay/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== description_candidate lib tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --lib description_candidate::tests -- --test-threads=1 \
  > /evidence/linux/preoverlay/test-lib.log 2>&1
lib_exit=$?
set -e
record test-lib "$lib_exit"

echo "=== description_candidate integration tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test description_candidate -- --test-threads=1 \
  > /evidence/linux/preoverlay/test-description.log 2>&1
description_exit=$?
set -e
record test-description "$description_exit"

echo "=== clippy description candidate surface ==="
if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/preoverlay/clippy-component.log 2>&1
  component_exit=$?
  set -e
  record clippy-component "$component_exit"
else
  printf '%s\n' "already-installed" > /evidence/linux/preoverlay/clippy-component.log
  record clippy-component 0
fi
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --lib --test description_candidate -- -D warnings \
  > /evidence/linux/preoverlay/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/preoverlay/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/preoverlay/owned-sha256.txt)
after=$(cat /evidence/linux/preoverlay/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/preoverlay/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/preoverlay/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/preoverlay/smoke.exit
echo "preedit_smoke=ok"
exit 0
