#!/bin/bash
# Re-extract the recorded Debian libclang set onto a fresh private cargo volume.
# Reads the smoke deb list. Does not rewrite smoke evidence.
# Origin: evidence/.../numeric-query/linux/smoke/extract-libclang.sh
set -euo pipefail
ROOT=/cargo-home/a327-clang-root
DEBS=/cargo-home/a327-debs
LIST=/evidence/linux/smoke/libclang-debs.txt
OUT=/evidence/linux/live-3
mkdir -p "$ROOT" "$DEBS" "$OUT"
if [ ! -s "$LIST" ]; then
  echo "recorded libclang deb list is missing" >&2
  exit 1
fi
while read -r name filename digest; do
  [ -n "$name" ] || continue
  deb="${DEBS}/${name}.deb"
  if [ ! -s "$deb" ]; then
    curl -fsSL "http://deb.debian.org/debian/${filename}" -o "$deb"
  fi
  got=$(/cargo-home/bin/sha256 file "$deb" | awk '{print $1}')
  if [ "$got" != "$digest" ]; then
    echo "checksum mismatch for $name" >&2
    exit 1
  fi
  dpkg-deb -x "$deb" "$ROOT"
done < "$LIST"
libdir=$(find "$ROOT/usr/lib" \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
test -n "$libdir"
dirname "$libdir" > "$OUT/libclang-dir.txt"
echo "libclang_extracted"
