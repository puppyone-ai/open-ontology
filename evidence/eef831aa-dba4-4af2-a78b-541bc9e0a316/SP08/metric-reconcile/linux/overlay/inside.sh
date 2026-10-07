#!/bin/bash
# Compile and test metric-reconcile against the read-only fixed archive.
# Child lock is generated only under tools/metric-reconcile. No cargo update.
set -u
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target/a316-metric
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
unset OO_A316_LIVE
mkdir -p /evidence/linux/overlay/phases /cargo-home/bin /target/a316-metric
cd /src

phase() {
  printf '%s\n' "$2" > "/evidence/linux/overlay/phases/$1.exit"
  echo "PHASE $1=$2"
}

echo "=== identity ==="
rustc -vV | tee /evidence/linux/overlay/rustc-verbose.txt
cargo -V | tee /evidence/linux/overlay/cargo-version.txt
uname -a | tee /evidence/linux/overlay/uname.txt
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/linux/overlay/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) phase toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; phase toolchain 2; exit 2 ;;
esac

if [ ! -f crates/oo-gateway/src/sql_guard.rs ] || [ ! -f tools/metric-reconcile/src/lib.rs ]; then
  echo "archive or crate source is missing" >&2
  phase source 2
  exit 2
fi
if [ ! -f examples/schema/metrics-finance.yaml ]; then
  echo "finance metric yaml is missing from the archive" >&2
  phase source 2
  exit 2
fi
phase source 0

if [ ! -x /cargo-home/bin/sha256 ]; then
  rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
  phase sha256-tool $?
else
  phase sha256-tool 0
fi
export PATH="/cargo-home/bin:${PATH}"

/cargo-home/bin/sha256 file Cargo.lock > /evidence/linux/overlay/lock-before.txt
before=$(awk '{print $1}' /evidence/linux/overlay/lock-before.txt)
printf '%s\n' "$before" > /evidence/linux/overlay/lock-before.sha256
if [ "$before" != "16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a" ]; then
  echo "root lock is not the preoverlay baseline" >&2
  phase lock-before 1
  exit 1
fi
phase lock-before 0

if [ ! -d /cargo-home/a316-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/overlay/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  phase libclang "$extract_exit"
  if [ "$extract_exit" -ne 0 ]; then
    exit "$extract_exit"
  fi
else
  echo "libclang_root=reused" > /evidence/linux/overlay/libclang-extract.log
  phase libclang 0
fi
libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a316-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a316-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a316-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a316-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a316-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/overlay/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$libdir" ]; then
  phase libclang-includes 1
  exit 1
fi
phase libclang-includes 0
printf '%s\n' "no" > /evidence/linux/overlay/cargo-update-invoked.txt

echo "=== cargo generate-lockfile --offline ==="
cd /src/tools/metric-reconcile
set +e
cargo generate-lockfile --offline > /evidence/linux/overlay/lock.stdout 2> /evidence/linux/overlay/lock.stderr
lock_exit=$?
set -e
printf '%s\n' offline > /evidence/linux/overlay/lock-mode.txt
if [ "$lock_exit" -ne 0 ]; then
  echo "=== cargo generate-lockfile online ==="
  printf '%s\n' online > /evidence/linux/overlay/lock-mode.txt
  set +e
  cargo generate-lockfile > /evidence/linux/overlay/lock-online.stdout 2> /evidence/linux/overlay/lock-online.stderr
  lock_exit=$?
  set -e
fi
phase lock "$lock_exit"
if [ "$lock_exit" -ne 0 ]; then
  exit "$lock_exit"
fi

awk '
  function flush() {
    if (checksum != "") print name " " version " " checksum
    name = ""
    version = ""
    checksum = ""
  }
  /^\[\[package\]\]/ { flush(); next }
  $1 == "name" { gsub(/"/, "", $3); name = $3 }
  $1 == "version" { gsub(/"/, "", $3); version = $3 }
  $1 == "checksum" { gsub(/"/, "", $3); checksum = $3 }
  END { flush() }
' /src/Cargo.lock | sort > /tmp/root-pins.txt
awk '
  function flush() {
    if (checksum != "") print name " " version " " checksum
    name = ""
    version = ""
    checksum = ""
  }
  /^\[\[package\]\]/ { flush(); next }
  $1 == "name" { gsub(/"/, "", $3); name = $3 }
  $1 == "version" { gsub(/"/, "", $3); version = $3 }
  $1 == "checksum" { gsub(/"/, "", $3); checksum = $3 }
  END { flush() }
' Cargo.lock | sort > /tmp/crate-pins.txt
comm -13 /tmp/root-pins.txt /tmp/crate-pins.txt > /evidence/linux/overlay/lock-drift.txt
if [ -s /evidence/linux/overlay/lock-drift.txt ]; then
  phase lock-drift 2
else
  phase lock-drift 0
fi

echo "=== cargo test ==="
set +e
cargo test --locked --offline -- --test-threads=1 > /evidence/linux/overlay/test.stdout 2> /evidence/linux/overlay/test.stderr
test_exit=$?
set -e
phase test "$test_exit"

echo "=== cargo clippy ==="
set +e
cargo clippy --version > /evidence/linux/overlay/clippy-version.txt 2>&1
clippy_bin=$?
set -e
if [ "$clippy_bin" -ne 0 ]; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/overlay/component.stdout 2> /evidence/linux/overlay/component.stderr
  component_exit=$?
  set -e
  phase component "$component_exit"
  if [ "$component_exit" -ne 0 ]; then
    phase clippy "$component_exit"
    exit "$component_exit"
  fi
else
  phase component 0
fi
set +e
cargo clippy --locked --offline --all-targets -- -D warnings > /evidence/linux/overlay/clippy.stdout 2> /evidence/linux/overlay/clippy.stderr
clippy_exit=$?
set -e
phase clippy "$clippy_exit"

/cargo-home/bin/sha256 file /src/Cargo.lock | awk '{print $1}' > /evidence/linux/overlay/lock-after.sha256
after=$(cat /evidence/linux/overlay/lock-after.sha256)
if [ "$before" != "$after" ]; then
  echo "root lock changed during overlay" >&2
  phase lock-hash 1
  lock_hash=1
else
  phase lock-hash 0
  lock_hash=0
fi

live_exit=0
if [ "$test_exit" -ne 0 ] || [ "$clippy_exit" -ne 0 ] || [ "$lock_hash" -ne 0 ]; then
  printf '%s\n' skipped > /evidence/linux/overlay/phases/live.exit
  echo "PHASE live=skipped"
else
  if [ ! -f /secrets/login.pass ] || [ ! -f /secrets/admin.pass ] || [ ! -f /secrets/ca.crt ]; then
    phase live 2
    live_exit=2
  else
    mkdir -p /tmp/a316-secrets
    cp /secrets/admin.pass /tmp/a316-secrets/admin.pass
    cp /secrets/login.pass /tmp/a316-secrets/login.pass
    cp /secrets/ca.crt /tmp/a316-secrets/ca.crt
    chmod 600 /tmp/a316-secrets/admin.pass /tmp/a316-secrets/login.pass
    chmod 644 /tmp/a316-secrets/ca.crt
    echo "=== live fixture ==="
    set +e
    OO_A316_LIVE=1 \
      OO_A316_HOST=oo-a316-pg \
      OO_A316_PORT=5432 \
      OO_A316_DATABASE=oo_a316 \
      OO_A316_ADMIN_USER=oo_fixture_admin \
      OO_A316_LOGIN_USER=oo_a316_login \
      OO_A316_ADMIN_PASSWORD_FILE=/tmp/a316-secrets/admin.pass \
      OO_A316_LOGIN_PASSWORD_FILE=/tmp/a316-secrets/login.pass \
      OO_A316_CA_FILE=/tmp/a316-secrets/ca.crt \
      cargo test --locked --offline --lib hand_literal_independent_control -- --test-threads=1 \
      > /evidence/linux/overlay/live.stdout 2> /evidence/linux/overlay/live.stderr
    live_exit=$?
    set -e
    phase live "$live_exit"
  fi
fi

worst=0
for code in "$lock_exit" "$test_exit" "$clippy_exit" "$lock_hash" "$live_exit"; do
  if [ "$code" -ne 0 ]; then
    worst=$code
  fi
done
if [ -s /evidence/linux/overlay/lock-drift.txt ] && [ "$worst" -eq 0 ]; then
  worst=2
fi
printf '%s\n' "$worst" > /evidence/linux/overlay/overlay.exit
echo "overlay_exit=$worst"
exit "$worst"
