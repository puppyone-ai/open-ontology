#!/bin/bash
# Focused smoke of the fixed description-candidate library and CLI.
# Linux arm64 Rust 1.94. No cargo update. No apt. No PostgreSQL. No model.
# Origin inside: a258 commit 1cd687d3f106fb9bde8c2a4a0b40cf2dddae9368
#   evidence/eef831aa-dba4-4af2-a78b-541bc9e0a258/SP07/description-candidate-cli/linux/preoverlay/inside.sh
#   blob 4bf98b9a1f39d60c83c23b1820faa3a041e9a055
# Changes: fixed 182e2137 already contains the CLI. The provider directory must
# be absent. Smoke the library, the CLI, and the selection-order test.
# Offline fetch is recorded first. A cold volume may fail that attempt.
set -u
echo "oo_task=sp07-description-provider-preoverlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a263"
echo "oo_fixed=182e2137ee261ed889b1aff5ada6e505a4a17723"
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

note() {
  mkdir -p /evidence/linux/preoverlay/phases
  printf '%s\n' "$2" > "/evidence/linux/preoverlay/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
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
if [ ! -f crates/oo-builder/src/bin/description-candidate.rs ]; then
  echo "fixed archive is missing the description-candidate CLI" >&2
  record source 2
fi
if [ ! -f contracts/builder/description-candidate-v1.md ]; then
  echo "fixed archive is missing the description candidate contract" >&2
  record source 2
fi
if [ ! -f contracts/builder/description-candidate-cli-v1.md ]; then
  echo "fixed archive is missing the description candidate CLI contract" >&2
  record source 2
fi
if [ -e tools/description-provider ]; then
  echo "description provider is already on the pre-edit archive" >&2
  record source 2
fi
printf '%s\n' "provider_absent" > /evidence/linux/preoverlay/overlay-absent.txt
record source 0

/cargo-home/bin/sha256 file \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/description_candidate.rs \
  crates/oo-builder/src/bin/description-candidate.rs \
  crates/oo-builder/Cargo.toml \
  contracts/builder/description-candidate-v1.md \
  contracts/builder/description-candidate-cli-v1.md \
  Cargo.toml \
  Cargo.lock \
  > /evidence/linux/preoverlay/owned-sha256.txt
record owned-hash $?
printf '%s\n' "no" > /evidence/linux/preoverlay/cargo-update-invoked.txt

echo "=== cargo fetch --locked --offline ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/linux/preoverlay/fetch-offline.log 2>&1
offline_exit=$?
set -e
note fetch-offline "$offline_exit"
if [ "$offline_exit" -ne 0 ]; then
  echo "=== cargo fetch --locked ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/preoverlay/fetch.log 2>&1
  fetch_exit=$?
  set -e
  record fetch "$fetch_exit"
  echo "=== cargo fetch --locked --offline ==="
  set +e
  cargo fetch --manifest-path Cargo.toml --locked --offline > /evidence/linux/preoverlay/fetch-offline-after.log 2>&1
  offline_after=$?
  set -e
  record fetch-offline-after "$offline_after"
else
  printf '%s\n' "offline-ok" > /evidence/linux/preoverlay/fetch.log
  record fetch 0
  record fetch-offline-after 0
fi

echo "=== description_candidate integration tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test description_candidate -- --test-threads=1 \
  > /evidence/linux/preoverlay/test-description.log 2>&1
description_exit=$?
set -e
record test-description "$description_exit"

echo "=== description_candidate CLI tests ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test description_candidate_cli -- --test-threads=1 \
  > /evidence/linux/preoverlay/test-cli.log 2>&1
cli_exit=$?
set -e
record test-cli "$cli_exit"

echo "=== description selection order ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder --test description_selection_order -- --test-threads=1 \
  > /evidence/linux/preoverlay/test-order.log 2>&1
order_exit=$?
set -e
record test-order "$order_exit"

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
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder --test description_candidate --test description_candidate_cli --test description_selection_order -- -D warnings \
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
