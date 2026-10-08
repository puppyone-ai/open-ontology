#!/bin/bash
# Re-extract the recorded Debian libclang set onto this task's cargo volume.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386/SP09/local-analysis/linux/chain-full/extract-libclang.sh
# sha256 fd99b47ad143551cecd737b464293318937333e1efc8142147dc3caa3e026243
# Changes: owner a401 cargo paths and proof output directory. Deb list and checksums are unchanged.
set -euo pipefail
ROOT=/cargo-home/a401-clang-root
DEBS=/cargo-home/a401-debs
LIST=/evidence/linux/libclang-debs.txt
OUT=/evidence/linux/proof
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
