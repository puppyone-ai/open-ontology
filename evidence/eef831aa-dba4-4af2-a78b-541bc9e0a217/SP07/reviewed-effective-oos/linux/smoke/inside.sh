#!/bin/bash
# Pre-overlay oo-builder smoke on the immutable c2db9c43 archive.
# Linux arm64 Rust 1.94. No product overlay. No cargo update. No apt. No PostgreSQL.
set -u
echo "oo_task=sp07-reviewed-effective-oos-preoverlay"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a217"
echo "oo_fixed=c2db9c430b822a824a1062b6c3c7345924df7dd8"
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
mkdir -p /evidence/linux/smoke/phases /cargo-home/bin
cd /src

record() {
  mkdir -p /evidence/linux/smoke/phases
  printf '%s\n' "$2" > "/evidence/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/smoke/smoke.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/smoke/rustc-version.txt
rustc -Vv > /evidence/linux/smoke/rustc-verbose.txt
cargo -V | tee /evidence/linux/smoke/cargo-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac

rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
record sha256-tool $?
export PATH="/cargo-home/bin:${PATH}"

if [ ! -f crates/oo-builder/src/lib.rs ]; then
  echo "fixed archive is missing oo-builder lib.rs" >&2
  record source 2
fi
if [ -f crates/oo-builder/src/reviewed_effective.rs ]; then
  echo "product overlay is already on the pre-overlay archive" >&2
  record source 2
fi
if [ -f crates/oo-builder/src/bin/reviewed-effective-oos.rs ]; then
  echo "product bin overlay is already on the pre-overlay archive" >&2
  record source 2
fi
if [ -f rust-toolchain.toml ]; then
  echo "extracted tree still selects a toolchain file" >&2
  record source 2
fi
printf '%s\n' "overlay_absent" > /evidence/linux/smoke/overlay-absent.txt

/cargo-home/bin/sha256 file \
  crates/oo-builder/src/lib.rs \
  crates/oo-builder/src/bin/oo-builder.rs \
  crates/oo-builder/src/role_pack.rs \
  crates/oo-builder/src/role_pack_cli.rs \
  crates/oo-builder/src/captured_pg.rs \
  contracts/builder/role-pack-v1.md \
  contracts/builder/captured-pg-draft-v1.md \
  Cargo.lock \
  > /evidence/linux/smoke/owned-sha256.txt
record owned-hash $?

printf '%s\n' "no" > /evidence/linux/smoke/cargo-update-invoked.txt

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/smoke/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  echo "clippy_component=already_installed" > /evidence/linux/smoke/clippy-component.log
  record clippy-component 0
fi

echo "=== cargo fetch --locked ==="
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

echo "=== cargo test focused oo-builder boundary ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-builder \
  --test captured_pg --test review_pg_cli --test role_pack --test role_pack_cli \
  -- --test-threads=1 \
  > /evidence/linux/smoke/test.log 2>&1
test_exit=$?
set -e
record test "$test_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/smoke/lock-after.sha256
before=$(awk '$2 ~ /Cargo.lock$/ {print $1}' /evidence/linux/smoke/owned-sha256.txt)
after=$(cat /evidence/linux/smoke/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "lock changed before=$before after=$after" > /evidence/linux/smoke/lock-hash.txt
  record lock-hash 1
fi
echo "unchanged $after" > /evidence/linux/smoke/lock-hash.txt
record lock-hash 0

printf '%s\n' 0 > /evidence/linux/smoke/smoke.exit
echo "preoverlay_smoke=ok"
exit 0
