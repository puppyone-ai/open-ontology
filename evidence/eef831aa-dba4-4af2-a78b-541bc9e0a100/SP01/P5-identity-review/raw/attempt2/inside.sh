#!/bin/bash
# a100 identity probe. Default-feature parent only. Does not modify /src.
set -u
mkdir -p /evidence/raw /opt/clangroot /opt/debs /opt/probe-build
phase() { printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >> /evidence/raw/phase.log; }
save_exit() { printf '%s\n' "$2" > "/evidence/raw/$1.exit"; phase "$1 exit $2"; }

phase "inside-start"

{
  echo "id=$(id)"
  rustc -vV
  cargo -V
  echo "protoc=$(command -v protoc || echo absent)"
  echo "---- /proc/1/status ----"
  awk '/^(CapEff|CapPrm|NoNewPrivs|PPid):/ {print}' /proc/1/status
  echo "---- /proc/self/status ----"
  awk '/^(CapEff|CapPrm|NoNewPrivs|PPid):/ {print}' /proc/self/status
} > /evidence/raw/toolchain.txt 2> /evidence/raw/toolchain.err
save_exit toolchain $?

phase "debs-start"
# Attempt 1 apt-get under CapDrop=ALL failed (setuid/setgroups). Debs are the
# a091 byte-identical files, mounted read-only. No apt in this attempt.
sha256sum /debs/*.deb > /evidence/raw/debs-sha256.txt
missing=0
while read -r sum name; do
  if [ -f "/debs/$name" ]; then
    got=$(sha256sum "/debs/$name" | awk '{print $1}')
    if [ "$got" = "$sum" ]; then
      echo "match $name" >> /evidence/raw/debs-compare.txt
    else
      echo "mismatch $name expected=$sum got=$got" >> /evidence/raw/debs-compare.txt
      missing=1
    fi
  else
    echo "absent $name" >> /evidence/raw/debs-compare.txt
    missing=1
  fi
done < /probe/debs.sha256
printf '%s\n' "$missing" > /evidence/raw/debs-compare.exit
if [ "$missing" -ne 0 ]; then
  phase "stop-deb-hash"
  printf '1\n' > /evidence/raw/inside.exit
  exit 1
fi
deb_rc=0
for deb in /debs/*.deb; do
  [ -f "$deb" ] || continue
  dpkg-deb -x "$deb" /opt/clangroot || deb_rc=$?
done
save_exit debs-extract "$deb_rc"
ln -sfn clang-19 /opt/clangroot/usr/bin/clang || true
export PATH="/opt/clangroot/usr/bin:/opt/clangroot/usr/lib/llvm-19/bin:${PATH:-/usr/bin:/bin}"
export LD_LIBRARY_PATH="/opt/clangroot/usr/lib/aarch64-linux-gnu:/opt/clangroot/usr/lib/llvm-19/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -e /opt/clangroot/usr/lib/llvm-19/lib/libclang.so ]; then
  export LIBCLANG_PATH=/opt/clangroot/usr/lib/llvm-19/lib
else
  mkdir -p /opt/clangroot/libclang
  so=$(find /opt/clangroot -name 'libclang-19.so.19' -o -name 'libclang.so.1' | head -1)
  ln -sfn "$so" /opt/clangroot/libclang/libclang.so
  export LIBCLANG_PATH=/opt/clangroot/libclang
fi
so=$(find /opt/clangroot -name 'libclang*.so*' | head -1)
if [ -n "$so" ]; then
  ldd "$so" > /evidence/raw/ldd-libclang.txt 2>&1
  ldd_rc=$?
else
  echo "libclang missing" > /evidence/raw/ldd-libclang.txt
  ldd_rc=1
fi
if grep -q 'not found' /evidence/raw/ldd-libclang.txt; then
  ldd_rc=1
fi
save_exit ldd "$ldd_rc"
clang --version > /evidence/raw/clang-version.txt 2>&1 || true
phase "debs-done"

sha256sum /src/Cargo.lock /src/corpus/expected.json /probe/Cargo.toml /probe/src/main.rs > /evidence/raw/input-sha256.txt
phase "input-hashed"

if [ "$ldd_rc" -ne 0 ]; then
  phase "stop-libclang"
  printf '1\n' > /evidence/raw/inside.exit
  exit 1
fi

cargo fetch --locked --manifest-path /src/Cargo.toml > /evidence/raw/fetch.out 2> /evidence/raw/fetch.err
fetch_rc=$?
save_exit fetch "$fetch_rc"
if [ "$fetch_rc" -ne 0 ]; then
  phase "stop-after-fetch-failure"
  printf '1\n' > /evidence/raw/inside.exit
  exit 1
fi

rm -rf /opt/probe-build
mkdir -p /opt/probe-build/src
cp /probe/Cargo.toml /opt/probe-build/Cargo.toml
cp /probe/src/main.rs /opt/probe-build/src/main.rs
cargo fetch --manifest-path /opt/probe-build/Cargo.toml > /evidence/raw/fetch-probe.out 2> /evidence/raw/fetch-probe.err
fetch_probe_rc=$?
save_exit fetch-probe "$fetch_probe_rc"
if [ -f /opt/probe-build/Cargo.lock ]; then
  cp /opt/probe-build/Cargo.lock /evidence/raw/probe-Cargo.lock
  sha256sum /evidence/raw/probe-Cargo.lock > /evidence/raw/probe-Cargo.lock.sha256
fi

cargo build --offline --manifest-path /opt/probe-build/Cargo.toml > /evidence/raw/build.out 2> /evidence/raw/build.err
build_rc=$?
save_exit build "$build_rc"
if [ "$build_rc" -eq 0 ] && [ -x /target/debug/p5-identity-probe ]; then
  sha256sum /target/debug/p5-identity-probe > /evidence/raw/probe-bin.sha256
  wc -c /target/debug/p5-identity-probe > /evidence/raw/probe-bin.bytes
  cp -a /target/debug/p5-identity-probe /opt/probe-build/p5-identity-probe
  phase "probe-built"
else
  phase "probe-missing"
fi

if [ "$build_rc" -eq 0 ] && [ -x /target/debug/p5-identity-probe ]; then
  timeout -k 20 240 /target/debug/p5-identity-probe --probe > /evidence/raw/probe.out 2> /evidence/raw/probe.err
  save_exit probe $?
else
  printf '127\n' > /evidence/raw/probe.exit
  phase "probe-skipped"
fi

final=0
for name in fetch fetch-probe build probe; do
  rc=$(cat "/evidence/raw/$name.exit" 2>/dev/null || echo missing)
  if [ "$rc" != "0" ]; then
    final=1
  fi
done
save_exit inside "$final"
exit "$final"
