#!/bin/bash
# Review build. Smoke and Clippy finish before the independent oracle.
# Author live test is not invoked. Fixed archive stays read-only.
set -eu
export RUSTUP_TOOLCHAIN=1.94.0
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TERM_COLOR=never
export CARGO_BUILD_JOBS=1
unset OO_A316_LIVE
unset OO_A329_EVIDENCE
mkdir -p /evidence/linux/review-3/phases /cargo-home/bin /evidence/linux/smoke
cd /src

phase() {
  printf '%s\n' "$2" > "/evidence/linux/review-3/phases/$1.exit"
  echo "PHASE $1=$2"
}

echo "=== identity ==="
rustc -vV | tee /evidence/linux/review-3/rustc-verbose.txt
cargo -V | tee /evidence/linux/review-3/cargo-version.txt
uname -a | tee /evidence/linux/review-3/uname.txt
got=$(rustc --version)
printf '%s\n' "$got" > /evidence/linux/review-3/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) phase toolchain 0 ;;
  *) echo "refusing toolchain: $got" >&2; phase toolchain 2; exit 2 ;;
esac

if [ ! -f crates/oo-gateway/src/sql_guard.rs ] || [ ! -f crates/oo-gateway/tests/sql_guard.rs ]; then
  echo "fixed archive is missing sql_guard" >&2
  phase source 2
  exit 2
fi
if [ ! -f tools/metric-reconcile/src/lib.rs ] || [ ! -f tools/metric-reconcile/Cargo.lock ]; then
  echo "fixed archive is missing metric-reconcile" >&2
  phase source 2
  exit 2
fi
phase source 0
printf '%s\n' not_invoked > /evidence/linux/review-3/author-live.txt

set +e
rustc --edition 2021 /tools/sha256.rs -o /cargo-home/bin/sha256
tool_rc=$?
set -e
phase sha256-tool "$tool_rc"
[ "$tool_rc" -eq 0 ]
set +e
rustc --edition 2021 /tools/select-debs.rs -o /cargo-home/bin/select-debs
tool_rc=$?
set -e
phase select-debs-tool "$tool_rc"
[ "$tool_rc" -eq 0 ]
export PATH="/cargo-home/bin:${PATH}"

/cargo-home/bin/sha256 file Cargo.lock > /evidence/linux/review-3/lock-before.txt
before=$(awk '{print $1}' /evidence/linux/review-3/lock-before.txt)
printf '%s\n' "$before" > /evidence/linux/review-3/lock-before.sha256
if [ "$before" != "16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a" ]; then
  echo "root lock is not the fixed-archive baseline" >&2
  phase lock-before 1
  exit 1
fi
phase lock-before 0
set +e
/cargo-home/bin/sha256 dir /evidence/linux/review-3/source-sha256.txt /src/tools/metric-reconcile
hash_rc=$?
set -e
phase source-hash "$hash_rc"
[ "$hash_rc" -eq 0 ]

if [ ! -d /cargo-home/a329-clang-root/usr/lib ]; then
  set +e
  bash /extract-libclang.sh > /evidence/linux/review-3/libclang-extract.log 2>&1
  extract_exit=$?
  set -e
  phase libclang "$extract_exit"
  if [ "$extract_exit" -ne 0 ]; then
    exit "$extract_exit"
  fi
else
  echo "libclang_root=reused" > /evidence/linux/review-3/libclang-extract.log
  phase libclang 0
fi
libdir=$(cat /evidence/linux/smoke/libclang-dir.txt)
multiarch=/cargo-home/a329-clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/a329-clang-root/usr/lib/llvm-19/lib/clang/19/include
devso=$(find /cargo-home/a329-clang-root/usr/lib -name 'libclang.so' -print -quit)
if [ -n "$devso" ]; then
  libdir=$(dirname "$devso")
fi
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/bin:/cargo-home/a329-clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/a329-clang-root/usr/share/pkgconfig"
{
  echo "LIBCLANG_PATH=$LIBCLANG_PATH"
  test -f "$llvm_inc/stdbool.h" && echo "stdbool.h=present" || echo "stdbool.h=absent"
} > /evidence/linux/review-3/libclang.txt
if [ ! -f "$llvm_inc/stdbool.h" ] || [ -z "$libdir" ]; then
  phase libclang-includes 1
  exit 1
fi
phase libclang-includes 0
printf '%s\n' "no" > /evidence/linux/review-3/cargo-update-invoked.txt

echo "=== cargo fetch workspace ==="
export CARGO_TARGET_DIR=/target/a329-smoke
set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/review-3/fetch.log 2>&1
fetch_exit=$?
set -e
phase fetch "$fetch_exit"
if [ "$fetch_exit" -ne 0 ]; then
  exit "$fetch_exit"
fi

echo "=== author smoke sql_guard ==="
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test sql_guard -- --test-threads=1 \
  > /evidence/linux/review-3/smoke.stdout 2> /evidence/linux/review-3/smoke.stderr
smoke_exit=$?
set -e
phase smoke "$smoke_exit"

/cargo-home/bin/sha256 file Cargo.lock | awk '{print $1}' > /evidence/linux/review-3/lock-after-smoke.sha256
if [ "$before" != "$(cat /evidence/linux/review-3/lock-after-smoke.sha256)" ]; then
  phase lock-smoke 1
  exit 1
fi
phase lock-smoke 0

echo "=== metric-reconcile fetch ==="
cd /src/tools/metric-reconcile
export CARGO_TARGET_DIR=/target/a329-metric
set +e
cargo fetch --locked --offline > /evidence/linux/review-3/metric-fetch.log 2>&1
metric_fetch=$?
set -e
if [ "$metric_fetch" -ne 0 ]; then
  echo "=== metric-reconcile fetch online ==="
  set +e
  cargo fetch --locked > /evidence/linux/review-3/metric-fetch-online.log 2>&1
  metric_fetch=$?
  set -e
  printf '%s\n' online > /evidence/linux/review-3/metric-fetch-mode.txt
else
  printf '%s\n' offline > /evidence/linux/review-3/metric-fetch-mode.txt
fi
phase metric-fetch "$metric_fetch"
if [ "$metric_fetch" -ne 0 ]; then
  exit "$metric_fetch"
fi

echo "=== cargo test metric-reconcile ==="
unset OO_A316_LIVE
set +e
cargo test --locked --offline -- --test-threads=1 \
  > /evidence/linux/review-3/test.stdout 2> /evidence/linux/review-3/test.stderr
test_exit=$?
set -e
phase test "$test_exit"

echo "=== cargo clippy ==="
set +e
cargo clippy --version > /evidence/linux/review-3/clippy-version.txt 2>&1
clippy_bin=$?
set -e
if [ "$clippy_bin" -ne 0 ]; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/review-3/component.stdout 2> /evidence/linux/review-3/component.stderr
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
cargo clippy --locked --offline --all-targets -- -D warnings \
  > /evidence/linux/review-3/clippy.stdout 2> /evidence/linux/review-3/clippy.stderr
clippy_exit=$?
set -e
phase clippy "$clippy_exit"

/cargo-home/bin/sha256 file /src/Cargo.lock | awk '{print $1}' > /evidence/linux/review-3/lock-after.sha256
if [ "$before" != "$(cat /evidence/linux/review-3/lock-after.sha256)" ]; then
  phase lock-hash 1
  lock_hash=1
else
  phase lock-hash 0
  lock_hash=0
fi

live_exit=0
oracle_exit=0
if [ "$smoke_exit" -ne 0 ] || [ "$test_exit" -ne 0 ] || [ "$clippy_exit" -ne 0 ] || [ "$lock_hash" -ne 0 ]; then
  printf '%s\n' skipped > /evidence/linux/review-3/phases/oracle.exit
  printf '%s\n' skipped > /evidence/linux/review-3/phases/live.exit
  echo "PHASE live=skipped"
else
  echo "=== oracle ==="
  rm -rf /tmp/oracle-build
  mkdir -p /tmp/oracle-build
  cp -a /oracle/. /tmp/oracle-build/
  cd /tmp/oracle-build
  export CARGO_TARGET_DIR=/target/a329-oracle
  set +e
  cargo generate-lockfile --offline > /evidence/linux/review-3/oracle-lock.stdout 2> /evidence/linux/review-3/oracle-lock.stderr
  oracle_lock=$?
  set -e
  printf '%s\n' offline > /evidence/linux/review-3/oracle-lock-mode.txt
  if [ "$oracle_lock" -ne 0 ]; then
    set +e
    cargo generate-lockfile > /evidence/linux/review-3/oracle-lock-online.stdout 2> /evidence/linux/review-3/oracle-lock-online.stderr
    oracle_lock=$?
    set -e
    printf '%s\n' online > /evidence/linux/review-3/oracle-lock-mode.txt
  fi
  phase oracle-lock "$oracle_lock"
  if [ "$oracle_lock" -ne 0 ]; then
    oracle_exit=$oracle_lock
  else
    cp Cargo.lock /evidence/linux/review-3/oracle-Cargo.lock
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
    ' Cargo.lock | sort > /tmp/oracle-pins.txt
    comm -13 /tmp/root-pins.txt /tmp/oracle-pins.txt > /evidence/linux/review-3/oracle-lock-drift.txt
    if [ -s /evidence/linux/review-3/oracle-lock-drift.txt ]; then
      phase oracle-drift 2
      oracle_exit=2
    else
      phase oracle-drift 0
      set +e
      cargo build --locked --offline > /evidence/linux/review-3/oracle-build.stdout 2> /evidence/linux/review-3/oracle-build.stderr
      oracle_exit=$?
      set -e
      phase oracle "$oracle_exit"
    fi
  fi
  if [ "$oracle_exit" -ne 0 ]; then
    printf '%s\n' skipped > /evidence/linux/review-3/phases/live.exit
    live_exit=$oracle_exit
  else
    if [ ! -f /secrets/login.pass ] || [ ! -f /secrets/admin.pass ] || [ ! -f /secrets/ca.crt ]; then
      phase live 2
      live_exit=2
    else
      mkdir -p /tmp/a329-secrets /evidence/linux/review-3/live
      cp /secrets/admin.pass /tmp/a329-secrets/admin.pass
      cp /secrets/login.pass /tmp/a329-secrets/login.pass
      cp /secrets/ca.crt /tmp/a329-secrets/ca.crt
      chmod 600 /tmp/a329-secrets/admin.pass /tmp/a329-secrets/login.pass
      chmod 644 /tmp/a329-secrets/ca.crt
      echo "=== independent oracle ==="
      set +e
      OO_A329_EVIDENCE=/evidence/linux/review-3/live \
        OO_A329_HOST=oo-a329-pg \
        OO_A329_PORT=5432 \
        OO_A329_DATABASE=oo_a329 \
        OO_A329_ADMIN_USER=oo_fixture_admin \
        OO_A329_LOGIN_USER=oo_a329_login \
        OO_A329_ADMIN_PASSWORD_FILE=/tmp/a329-secrets/admin.pass \
        OO_A329_LOGIN_PASSWORD_FILE=/tmp/a329-secrets/login.pass \
        OO_A329_CA_FILE=/tmp/a329-secrets/ca.crt \
        /target/a329-oracle/debug/a329-metric-review \
        > /evidence/linux/review-3/live.stdout 2> /evidence/linux/review-3/live.stderr
      live_exit=$?
      set -e
      phase live "$live_exit"
    fi
  fi
fi

/cargo-home/bin/sha256 file /src/Cargo.lock | awk '{print $1}' > /evidence/linux/review-3/lock-final.sha256
worst=0
for code in "$fetch_exit" "$smoke_exit" "$test_exit" "$clippy_exit" "$lock_hash" "$live_exit"; do
  if [ "$code" -ne 0 ]; then
    worst=$code
  fi
done
printf '%s\n' "$worst" > /evidence/linux/review-3/review.exit
echo "review_exit=$worst"
exit "$worst"
