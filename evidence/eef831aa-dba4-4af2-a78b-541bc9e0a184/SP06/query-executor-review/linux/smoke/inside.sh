#!/bin/bash
# Pre-overlay smoke. Immutable archive only. No review test. No PostgreSQL.
set -euo pipefail
echo "oo_task=sp06-query-executor-review-smoke"
echo "oo_uuid=eef831aa-dba4-4af2-a78b-541bc9e0a184"
echo "oo_toolchain_request=1.94.0"
echo "oo_platform=linux/arm64"
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export CARGO_TERM_COLOR=never
export HOME=/tmp
export CARGO_BUILD_JOBS=4
mkdir -p /evidence/linux/smoke/phases
rustc -Vv | tee /evidence/linux/smoke/rustc-version.txt
cargo -V | tee /evidence/linux/smoke/cargo-version.txt
got=$(rustc --version)
printf '%s\n' "$got"
case "$got" in
  "rustc 1.94.0 "*) ;;
  *) echo "refusing toolchain: $got" >&2; exit 2 ;;
esac
cd /src
if [ ! -f crates/oo-gateway/src/query_executor.rs ]; then
  echo "query_executor.rs missing" >&2
  exit 2
fi
if [ -f crates/oo-gateway/tests/query_executor_a184_review.rs ]; then
  echo "smoke archive already has the review overlay" >&2
  exit 2
fi
printf '%s\n' absent > /evidence/linux/smoke/overlay-absent.txt
if grep -n 'unsafe' crates/oo-gateway/src/query_executor.rs > /evidence/linux/smoke/unsafe-lines.txt; then
  echo "query_executor.rs contains unsafe" >&2
  exit 2
fi
printf '%s\n' 0 > /evidence/linux/smoke/phases/unsafe.exit
python3 - <<'PY'
import re
from pathlib import Path
text = Path("crates/oo-gateway/src/query_executor.rs").read_text()
names = ["run_producer", "drive", "read_pages"]
lines = []
for name in names:
    marker = re.search(
        rf"#\[allow\(clippy::too_many_arguments\)\]\s*async fn {name}(?:<[^>\n]+>)?\(",
        text,
    )
    if not marker:
        raise SystemExit(f"missing {name}")
    depth = 1
    index = marker.end()
    while index < len(text) and depth:
        char = text[index]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
        index += 1
    if depth:
        raise SystemExit(f"unclosed {name}")
    body = text[marker.end():index - 1]
    nest = 0
    args = []
    current = []
    for char in body:
        if char == "," and nest == 0:
            args.append("".join(current).strip())
            current = []
            continue
        if char == "<":
            nest += 1
        elif char == ">":
            nest = max(0, nest - 1)
        current.append(char)
    tail = "".join(current).strip()
    if tail:
        args.append(tail)
    lines.append(f"{name} allow=yes args={len(args)}")
    if len(args) <= 7:
        raise SystemExit(f"allow site is not truthful for {name}: {len(args)}")
owned = len(re.findall(r"classify_pg\([^)]*false\)", text))
lines.append(f"classify_pg_false_callsites={owned}")
Path("/evidence/linux/smoke/allow-sites.txt").write_text("\n".join(lines) + "\n")
print("\n".join(lines))
PY

so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
if [ -z "$so" ]; then
  set +e
  bash /extract-libclang.sh
  extract_exit=$?
  set -e
  printf '%s\n' "$extract_exit" > /evidence/linux/smoke/phases/libclang.exit
  if [ "$extract_exit" -ne 0 ]; then
    exit "$extract_exit"
  fi
  so=$(find /cargo-home/clang-root/usr/lib -name 'libclang.so' -print -quit || true)
else
  printf '%s\n' 0 > /evidence/linux/smoke/phases/libclang.exit
  printf '%s\n' reused > /evidence/linux/smoke/libclang-reused.txt
fi
if [ -z "$so" ]; then
  echo "libclang.so missing" >&2
  exit 2
fi
libdir=$(dirname "$so")
printf '%s\n' "$libdir" > /cargo-home/clang-root/libclang-dir.txt
multiarch=/cargo-home/clang-root/usr/lib/aarch64-linux-gnu
llvm_inc=/cargo-home/clang-root/usr/lib/llvm-19/lib/clang/19/include
export LIBCLANG_PATH="$libdir"
export LD_LIBRARY_PATH="${libdir}:${multiarch}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${llvm_inc} -I/usr/lib/gcc/aarch64-linux-gnu/14/include -I/usr/include/aarch64-linux-gnu -I/usr/include"
export PATH="/cargo-home/clang-root/usr/bin:${PATH}"
export PKG_CONFIG_PATH="${libdir}/pkgconfig:${multiarch}/pkgconfig:/cargo-home/clang-root/usr/share/pkgconfig"
printf '%s\n' "$LIBCLANG_PATH" > /evidence/linux/smoke/libclang-path.txt
if [ ! -e "$so" ]; then
  echo "libclang.so missing" >&2
  exit 2
fi
if [ ! -f "$llvm_inc/stdbool.h" ]; then
  echo "stdbool.h missing" >&2
  exit 2
fi
/lib/ld-linux-aarch64.so.1 --list "$so" > /evidence/linux/smoke/libclang-ldd.txt
if grep -q 'not found' /evidence/linux/smoke/libclang-ldd.txt; then
  echo "libclang did not resolve LLVM" >&2
  exit 2
fi
if ! grep -q 'libLLVM' /evidence/linux/smoke/libclang-ldd.txt; then
  echo "libclang did not resolve LLVM" >&2
  exit 2
fi
printf '%s\n' 0 > /evidence/linux/smoke/phases/libclang-load.exit

record() {
  printf '%s\n' "$2" > "/evidence/linux/smoke/phases/$1.exit"
  echo "phase_exit=$2 phase=$1"
  if [ "$2" -ne 0 ]; then
    exit "$2"
  fi
}

set +e
cargo fetch --manifest-path Cargo.toml --locked > /evidence/linux/smoke/fetch.log 2>&1
fetch_exit=$?
set -e
record fetch "$fetch_exit"

set +e
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib query_executor \
  > /evidence/linux/smoke/lib-test.log 2>&1
lib_exit=$?
set -e
record lib-test "$lib_exit"

if ! rustup component list --toolchain 1.94.0 --installed | grep -q '^clippy'; then
  set +e
  rustup component add clippy --toolchain 1.94.0 > /evidence/linux/smoke/clippy-component.log 2>&1
  clippy_add=$?
  set -e
  record clippy-component "$clippy_add"
else
  printf '%s\n' present > /evidence/linux/smoke/clippy-component.log
  record clippy-component 0
fi

set +e
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test query_executor_pg_live -- -D warnings \
  > /evidence/linux/smoke/clippy.log 2>&1
clippy_exit=$?
set -e
record clippy "$clippy_exit"
echo "smoke_phases_ok"
