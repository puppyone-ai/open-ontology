#!/bin/bash
# Fresh oracle against the archive copy plus the review test. No cargo update.
set -euo pipefail
echo "oo_task=sp06-query-executor-review-probe"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a184"
echo "oo_fixed=8a289f0c699abb8c707ca45ad45cb036abc3fc05"
echo "oo_platform=linux/arm64"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
export OO_A184_LIVE=1
export OO_A184_ORACLE=/evidence/live/oracle.txt
mkdir -p /evidence/linux/probe/phases /evidence/live
cd /src

record() {
  printf '%s\n' "$2" > "/evidence/linux/probe/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    printf '%s\n' "$2" > /evidence/linux/probe/probe.exit
    exit "$2"
  fi
}

got=$(rustc --version)
printf '%s\n' "$got" | tee /evidence/linux/probe/rustc-version.txt
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; record toolchain 2 ;;
esac
if [ ! -f crates/oo-gateway/tests/query_executor_a184_review.rs ]; then
  echo "review overlay is absent from the probe copy" >&2
  record overlay 2
fi
if grep -n 'unsafe' crates/oo-gateway/tests/query_executor_a184_review.rs > /evidence/linux/probe/unsafe-lines.txt; then
  echo "review overlay contains unsafe" >&2
  record unsafe 1
fi
printf '%s\n' 0 > /evidence/linux/probe/phases/unsafe.exit
sha256sum crates/oo-gateway/src/query_executor.rs > /evidence/linux/probe/query-executor.sha256
want=$(awk '$2=="./crates/oo-gateway/src/query_executor.rs" {print $1}' /evidence/source/blobs.sha256)
got_sha=$(awk '{print $1}' /evidence/linux/probe/query-executor.sha256)
if [ "$want" != "$got_sha" ]; then
  echo "probe query_executor.rs does not match the fixed blob" >&2
  record source 2
fi
printf '%s\n' "match" > /evidence/linux/probe/source-match.txt

so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
if [ -z "$so" ]; then
  echo "libclang.so missing from the smoke cargo volume" >&2
  record libclang 2
fi
libdir=$(dirname "$so")
multiarch=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/clang-root/usr/share/pkgconfig"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/probe/libclang-path.txt
record libclang 0

rm -f /evidence/live/oracle.txt
set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_executor_a184_review -- --nocapture --test-threads=1 \
  > /evidence/linux/probe/oracle-test.log 2>&1
test_exit=$?
set -e
record oracle-test "$test_exit"

python3 - <<'PY'
from pathlib import Path
oracle = Path("/evidence/live/oracle.txt")
if not oracle.is_file() or oracle.stat().st_size < 32:
    raise SystemExit("oracle missing")
text = oracle.read_text()
for name in ("admin.pass", "login.pass"):
    secret = Path("/run/oo-a184/fixture", name).read_text().strip()
    if secret and secret in text:
        raise SystemExit("oracle contains a secret")
if "SELECT " in text or "INSERT " in text:
    raise SystemExit("oracle contains SQL")
needed = [
    "typed_compare=exact",
    "audit_matches_plan=yes",
    "rollback_xact_cleared=yes",
    "role_reuse_audit_delta=0",
    "late_cancel=not_sent",
    "inflight_cancel=sent",
    "function_whitelist=not_claimed",
]
missing = [item for item in needed if item not in text]
if missing:
    raise SystemExit("oracle missing " + ",".join(missing))
print("oracle_scan=clean lines=" + str(text.count("\n")))
PY
printf '%s\n' 0 > /evidence/linux/probe/phases/oracle-scan.exit

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/probe/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  printf '%s\n' present > /evidence/linux/probe/clippy-component.log
  record clippy-component 0
fi

set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test query_executor_a184_review -- -D warnings \
  > /evidence/linux/probe/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"
echo "probe_phases_ok"
printf '%s\n' 0 > /evidence/linux/probe/probe.exit
