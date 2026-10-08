#!/bin/bash
# One Rust 1.94 builder. Extracts the pinned arm64 libclang debs, then builds
# the locked gateway, role-pack, and dialogue binaries. No cargo update.
set -u
OUT=/evidence/linux/build
BINS=/bins
mkdir -p "$OUT/phases" "$BINS"
export HOME=/tmp
export CARGO_HOME=/cargo-home
export CARGO_TARGET_DIR=/target
export RUSTUP_TOOLCHAIN=1.94.0
export CARGO_BUILD_JOBS=1
export CARGO_PROFILE_DEV_DEBUG=0
printf '%s\n' no > "$OUT/cargo-update-invoked.txt"
rustc --version > "$OUT/rustc-version.txt"
uname -m > "$OUT/uname.txt"

phase() {
  local name="$1"
  shift
  "$@" > "$OUT/${name}.log" 2>&1
  local code=$?
  printf '%s\n' "$code" > "$OUT/phases/${name}.exit"
  printf '%s %s\n' "$name" "$code"
  return "$code"
}

ROOT=/cargo-home/a399-clang-root
mkdir -p "$ROOT/debs"
extract_clang() {
  local base=http://deb.debian.org/debian
  local name pool sha dest
  while read -r name pool sha; do
    [ -n "$name" ] || continue
    dest="$ROOT/debs/$name"
    if [ ! -f "$dest" ] || ! printf '%s  %s\n' "$sha" "$dest" | sha256sum -c - >/dev/null 2>&1; then
      curl -fsSL --retry 2 -o "$dest.partial" "$base/$pool"
      mv "$dest.partial" "$dest"
    fi
    printf '%s  %s\n' "$sha" "$dest" | sha256sum -c -
    dpkg -x "$dest" "$ROOT"
  done < "$OUT/libclang-debs.txt"
  local so
  so=$(find "$ROOT" -name 'libclang.so' -o -name 'libclang-19.so' -o -name 'libclang-19.so.1' | head -n 1)
  if [ -z "$so" ]; then
    echo "libclang shared library missing" >&2
    return 1
  fi
  local dir
  dir=$(dirname "$so")
  if [ ! -e "$dir/libclang.so" ]; then
    ln -s "$(basename "$so")" "$dir/libclang.so"
  fi
  printf '%s\n' "$dir" > "$OUT/libclang-dir.txt"
  find "$dir" -maxdepth 1 -name 'libclang*' -o -name 'libLLVM*' | sort > "$OUT/libclang-names.txt"
}
phase extract-clang extract_clang || { printf '%s\n' 1 > "$OUT/build.exit"; exit 1; }

LIBDIR=$(cat "$OUT/libclang-dir.txt")
export LIBCLANG_PATH="$LIBDIR"
export LD_LIBRARY_PATH="$LIBDIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
CLANG_INCLUDE=$(find "$ROOT" -type d -path '*/clang/19/include' | head -n 1)
GCC_INCLUDE=$(find /usr/lib/gcc/aarch64-linux-gnu -type d -name include | head -n 1)
if [ -z "$CLANG_INCLUDE" ] || [ ! -f "$CLANG_INCLUDE/stdbool.h" ] || [ ! -f /usr/include/stdio.h ]; then
  printf 'clang_include=%s\ngcc_include=%s\n' "$CLANG_INCLUDE" "$GCC_INCLUDE" > "$OUT/clang-include.txt"
  printf '%s\n' 1 > "$OUT/build.exit"
  exit 1
fi
printf '%s\n%s\n' "$CLANG_INCLUDE" "$GCC_INCLUDE" > "$OUT/clang-include.txt"
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-unknown-linux-gnu -I${CLANG_INCLUDE} -I${GCC_INCLUDE} -I/usr/include"

build_gateway() {
  cargo build --manifest-path /work/Cargo.toml --locked --offline \
    -p oo-gateway --bin query-pg --bin install-column-acl --bin provision-mapping-reader
}
build_pack() {
  cargo build --manifest-path /work/Cargo.toml --locked --offline \
    -p oo-builder --bin role-pack-bundle
}
build_dialogue() {
  cargo build --manifest-path /work/tools/ontology-dialogue/Cargo.toml --locked --offline \
    --bin ontology-dialogue
}
build_tools() {
  rustc --edition 2021 -O -o "$BINS/forward" /evidence/linux/build/forward.rs
  rustc --edition 2021 -O -o "$BINS/live-tool" /evidence/linux/build/live-tool.rs
}

phase build-gateway build_gateway || { printf '%s\n' 1 > "$OUT/build.exit"; exit 1; }
phase build-pack build_pack || { printf '%s\n' 1 > "$OUT/build.exit"; exit 1; }
phase build-dialogue build_dialogue || { printf '%s\n' 1 > "$OUT/build.exit"; exit 1; }
phase build-tools build_tools || { printf '%s\n' 1 > "$OUT/build.exit"; exit 1; }

cp /target/debug/query-pg /target/debug/install-column-acl /target/debug/provision-mapping-reader \
  /target/debug/role-pack-bundle /target/debug/ontology-dialogue "$BINS/"
sha256sum "$BINS/query-pg" "$BINS/install-column-acl" "$BINS/provision-mapping-reader" \
  "$BINS/role-pack-bundle" "$BINS/ontology-dialogue" "$BINS/forward" "$BINS/live-tool" \
  > "$OUT/bin-sha256.txt"
printf '%s\n' 0 > "$OUT/build.exit"
exit 0
