#!/bin/bash
# Overlay proof. Fixed archive plus the reviewed-effective files only.
# Linux arm64 Rust 1.94. No cargo update. No apt. No PostgreSQL.
set -u
echo "oo_task=sp07-reviewed-effective-oos-overlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a217"
echo "oo_fixed=c2db9c430b822a824a1062b6c3c7345924df7dd8"
echo "oo_platform=linux/arm64"
echo "phase=overlay"
uname -a
cat /etc/os-release
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=1
mkdir -p /evidence/linux/overlay/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/overlay/phases
  printf '%s\n' "$2" > "/evidence/linux/overlay/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/overlay/overlay.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/overlay/rustc-version.txt
rustc -Vv > /evidence/linux/overlay/rustc-verbose.txt
cargo -V | tee /evidence/linux/overlay/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"

for path in \
  crates/oo-builder/src/reviewed_effective.rs \
  crates/oo-builder/src/bin/reviewed-effective-oos.rs \
  crates/oo-builder/tests/reviewed_effective.rs \
  contracts/builder/reviewed-effective-oos-v1.md
do
  if [ ! -f "$path" ]; then
    echo "overlay missing $path" >&2
    record source 2
  fi
done
if [ -f rust-toolchain.toml ]; then
  echo "extracted tree still selects a toolchain file" >&2
  record source 2
fi
if ! grep -q 'mod reviewed_effective;' crates/oo-builder/src/lib.rs; then
  echo "lib.rs overlay is missing mod reviewed_effective" >&2
  record source 2
fi
printf '%s\n' "overlay_present" > /evidence/linux/overlay/overlay-present.txt

/cargo-home/bin/sha256 file \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/reviewed_effective.rs \
  crates/oo-builder/src/bin/reviewed-effective-oos.rs \
  crates/oo-builder/src/bin/oo-builder.rs \
  crates/oo-builder/tests/reviewed_effective.rs \
  contracts/builder/reviewed-effective-oos-v1.md \
  Cargo.lock \
  > /evidence/linux/overlay/compiled-sha256.txt
record owned-hash $?

printf '%s\n' "no" > /evidence/linux/overlay/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/overlay/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/overlay/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/overlay/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test --lib reviewed_effective ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --lib reviewed_effective -- --test-threads=1 \
  > /evidence/linux/overlay/lib-test.log 2>&1
lib_exit=$?
set -e
record lib-test "$lib_exit"

echo "=== cargo test --test reviewed_effective ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --test reviewed_effective -- --test-threads=1 \
  > /evidence/linux/overlay/cli-test.log 2>&1
cli_exit=$?
set -e
record cli-test "$cli_exit"

echo "=== cargo clippy reviewed effective ==="
set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --lib --bins --test reviewed_effective -- -D warnings \
  > /evidence/linux/overlay/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/overlay/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/overlay/compiled-sha256.txt)
after=$(cat /evidence/linux/overlay/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/overlay/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/overlay/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/overlay/overlay.exit
echo "overlay_proof=ok"
exit 0
