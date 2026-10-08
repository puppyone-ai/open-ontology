#!/bin/bash
# One Rust 1.94 builder. No login shell. Product tree is read-only.
set -uo pipefail
cd /repo
mkdir -p /evidence/raw /evidence/linux/proof/phases /tmp/oo-a380-src
exec > >(tee -a /evidence/raw/console.log) 2>&1

echo "phase=toolchain"
rustc --version | tee /evidence/preedit-smoke.txt
cargo --version | tee -a /evidence/preedit-smoke.txt
rustc -vV | tee -a /evidence/preedit-smoke.txt
if ! rustc --version | grep -q 'rustc 1.94.0'; then
  echo "toolchain is not 1.94.0"
  exit 1
fi
cat > /tmp/oo-a380-smoke.rs << 'EOF'
use std::fs::OpenOptions;
fn main() {
    let path = std::env::temp_dir().join("oo-a380-lock");
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
    file.try_lock().unwrap();
    println!("try_lock=ok");
}
EOF
if ! rustc --edition 2021 /tmp/oo-a380-smoke.rs -o /cargo-home/bin-smoke 2>/evidence/linux/proof/phases/preedit-smoke.log; then
  printf '%s\n' 1 > /evidence/linux/proof/phases/preedit-smoke.exit
  exit 1
fi
/cargo-home/bin-smoke | tee -a /evidence/preedit-smoke.txt
printf '%s\n' 0 > /evidence/linux/proof/phases/preedit-smoke.exit

export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_BUILD_JOBS=1
export PATH="/usr/local/cargo/bin:/usr/bin:/bin"
export RUSTUP_HOME=/usr/local/rustup
export RUSTUP_TOOLCHAIN=1.94.0
git config --global --add safe.directory /repo || true

if ! cargo clippy --version >/evidence/linux/proof/phases/clippy-version.txt 2>&1; then
  rustup component add clippy
  cargo clippy --version
fi

fail=0
phase() {
  local name="$1"
  shift
  echo "phase=${name}"
  if "$@" >"/evidence/linux/proof/phases/${name}.log" 2>&1; then
    printf '%s\n' 0 >"/evidence/linux/proof/phases/${name}.exit"
  else
    local status=$?
    printf '%s\n' "$status" >"/evidence/linux/proof/phases/${name}.exit"
    echo "phase=${name} exit=${status}"
    fail=1
  fi
}

immutable=(
  crates/oo-gateway/src/query_executor.rs
  crates/oo-gateway/src/sql_guard.rs
  crates/oo-gateway/src/lib.rs
  crates/oo-gateway/src/policy.rs
  crates/oo-gateway/Cargo.toml
  Cargo.toml
  Cargo.lock
  tools/gateway-audit-wal/src/lib.rs
  tools/gateway-audit-wal/src/store.rs
  tools/audit-ingest/src/lib.rs
  tools/audit-ingest/src/pg.rs
)
if ! sha256sum "${immutable[@]}" > /evidence/raw/immutable-before.txt; then
  echo "immutable hash failed"
  exit 1
fi

if [ ! -f tools/gateway-audit-wal/Cargo.lock ] || [ ! -f tools/audit-ingest/Cargo.lock ]; then
  echo "child lock missing; not generating"
  exit 1
fi
printf '%s\n' 0 > /evidence/linux/proof/phases/lock-wal.exit
printf '%s\n' 0 > /evidence/linux/proof/phases/lock-ingest.exit
echo "phase=lock present"

phase fetch-wal cargo fetch --locked --manifest-path tools/gateway-audit-wal/Cargo.toml
phase fetch-ingest cargo fetch --locked --manifest-path tools/audit-ingest/Cargo.toml

mkdir -p /cargo-home/bin
phase sha256-tool rustc --edition 2021 /repo/deploy/trusted-demo/tools/sha256.rs -o /cargo-home/bin/sha256
phase select-debs-tool rustc --edition 2021 /repo/deploy/trusted-demo/tools/select-debs.rs -o /cargo-home/bin/select-debs
export OO_DEMO_PHASE=/evidence/linux/proof/phases
phase libclang-extract bash /repo/deploy/trusted-demo/linux/extract-libclang.sh
libdir=$(find /cargo-home/demo-clang-root/usr/lib \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit || true)
if [ -z "$libdir" ]; then
  echo "libclang shared object is absent"
  fail=1
else
  libdir=$(dirname "$libdir")
  multiarch=/cargo-home/demo-clang-root/usr/lib/aarch64-linux-gnu
  llvm_inc=$(find /cargo-home/demo-clang-root/usr/lib -type d -path '*/lib/clang/*/include' -print -quit || true)
  gcc_inc=$(find /usr/lib/gcc/aarch64-linux-gnu -type d -name include -print -quit || true)
  if [ -z "$llvm_inc" ] || [ -z "$gcc_inc" ]; then
    echo "libclang include path is absent"
    fail=1
  else
    export LIBCLANG_PATH="$libdir"
    export LD_LIBRARY_PATH="${libdir}:${multiarch}"
    export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I${gcc_inc} -I/usr/include/aarch64-linux-gnu -I/usr/include"
    export PATH="/cargo-home/bin:${PATH}"
    printf 'libdir=%s\nllvm_inc=%s\ngcc_inc=%s\n' "$libdir" "$llvm_inc" "$gcc_inc" > /evidence/linux/proof/phases/libclang-env.txt
    echo "phase=libclang ok"
  fi
fi
export CARGO_NET_OFFLINE=true

if [ "$fail" -eq 0 ]; then
  phase test-wal cargo test --locked --offline --manifest-path tools/gateway-audit-wal/Cargo.toml -- --test-threads=1
  phase test-ingest cargo test --locked --offline --manifest-path tools/audit-ingest/Cargo.toml -- --test-threads=1
  phase clippy-wal cargo clippy --locked --offline --all-targets --manifest-path tools/gateway-audit-wal/Cargo.toml -- -D warnings
  phase clippy-ingest cargo clippy --locked --offline --all-targets --manifest-path tools/audit-ingest/Cargo.toml -- -D warnings
  phase build-ingest cargo build --locked --offline --manifest-path tools/audit-ingest/Cargo.toml --bins
fi

if [ "$fail" -eq 0 ]; then
  rm -rf /tmp/oo-a380-src
  mkdir -p /tmp/oo-a380-src/tools /tmp/oo-a380-src/crates
  cp -a /repo/tools/gateway-audit-wal /tmp/oo-a380-src/tools/gateway-audit-wal
  if ! diff -rq /repo/tools/gateway-audit-wal /tmp/oo-a380-src/tools/gateway-audit-wal > /evidence/raw/copy-diff.txt; then
    echo "product copy drifted before the boundary file"
    fail=1
  else
    cp /evidence/linux/proof/boundary.rs /tmp/oo-a380-src/tools/gateway-audit-wal/tests/boundary.rs
    ln -s /repo/crates/oo-gateway /tmp/oo-a380-src/crates/oo-gateway
    ln -s /repo/crates/oo-core /tmp/oo-a380-src/crates/oo-core
    ln -s /repo/Cargo.toml /tmp/oo-a380-src/Cargo.toml
    ln -s /repo/Cargo.lock /tmp/oo-a380-src/Cargo.lock
    export OO_A380_BOUNDARY=1
    export OO_A380_REPO=/repo
    export OO_A380_RESULT=/evidence/raw/boundary-result.txt
    export OO_A380_INGEST_BIN=/target/debug/audit-ingest
    phase boundary cargo test --locked --offline --manifest-path /tmp/oo-a380-src/tools/gateway-audit-wal/Cargo.toml --test boundary -- --test-threads=1 --nocapture
  fi
fi

sha256sum "${immutable[@]}" > /evidence/raw/immutable-after.txt
if ! cmp -s /evidence/raw/immutable-before.txt /evidence/raw/immutable-after.txt; then
  echo "immutable files changed"
  fail=1
fi
if [ ! -f /evidence/raw/boundary-result.txt ] || ! grep -qx 'boundary=ok' /evidence/raw/boundary-result.txt; then
  echo "boundary result is not ok"
  fail=1
fi

secret_scan() {
  local needle line
  for needle in /run/secrets/admin.pass /run/secrets/login.pass /run/secrets/ingest.pass; do
    if grep -R -F -q -f "$needle" /evidence; then
      echo "secret scan failed"
      return 1
    fi
  done
  for needle in /run/secrets/mtls/server.key /run/secrets/mtls/client.key /run/secrets/mtls/stranger.key; do
    while IFS= read -r line || [ -n "$line" ]; do
      case "$line" in
        ""|-----BEGIN*|-----END*) continue ;;
      esac
      if [ "${#line}" -lt 40 ]; then
        continue
      fi
      if grep -R -F -q -- "$line" /evidence; then
        echo "secret scan failed"
        return 1
      fi
    done < "$needle"
  done
  echo "secret_scan=ok"
}
secret_scan
scan_status=$?
printf '%s\n' "$scan_status" > /evidence/linux/proof/phases/secret-scan.exit
if [ "$scan_status" -ne 0 ]; then
  fail=1
fi

printf '%s\n' "$fail" > /evidence/raw/inside.exit
echo "inside_exit=${fail}"
exit "$fail"
