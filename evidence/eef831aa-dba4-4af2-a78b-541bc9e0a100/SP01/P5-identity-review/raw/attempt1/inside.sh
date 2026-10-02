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
export DEBIAN_FRONTEND=noninteractive
apt-get update > /evidence/raw/apt-update.log 2>&1
save_exit apt-update $?
cd /opt/debs
exact=(
  clang-19=19.1.7-3+b1
  libclang-19-dev=19.1.7-3+b1
  libclang-common-19-dev=19.1.7-3+b1
  libclang-cpp19=19.1.7-3+b1
  libclang-rt-19-dev=19.1.7-3+b1
  libclang1-19=19.1.7-3+b1
  libllvm19=19.1.7-3+b1
  libz3-4=4.13.3-1
  llvm-19-linker-tools=19.1.7-3+b1
)
apt-get download "${exact[@]}" > /evidence/raw/apt-download.log 2>&1
dl_rc=$?
if [ "$dl_rc" -ne 0 ] || ! compgen -G '/opt/debs/*.deb' > /dev/null; then
  phase "debs-exact-failed-trying-unversioned"
  apt-get download \
    clang-19 libclang-19-dev libclang-common-19-dev libclang-cpp19 \
    libclang-rt-19-dev libclang1-19 libllvm19 libz3-4 llvm-19-linker-tools \
    >> /evidence/raw/apt-download.log 2>&1
  dl_rc=$?
fi
save_exit apt-download "$dl_rc"
sha256sum /opt/debs/*.deb > /evidence/raw/debs-sha256.txt 2>/evidence/raw/debs-sha256.err || true
# Compare to the a091 byte list when names match. A mismatch is recorded and is not rewritten.
if [ -f /probe/debs.sha256 ]; then
  missing=0
  while read -r sum name; do
    if [ -f "/opt/debs/$name" ]; then
      got=$(sha256sum "/opt/debs/$name" | awk '{print $1}')
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
fi
deb_rc=0
for deb in /opt/debs/*.deb; do
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
