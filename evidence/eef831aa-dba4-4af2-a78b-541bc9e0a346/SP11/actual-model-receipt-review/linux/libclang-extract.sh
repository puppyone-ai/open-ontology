#!/bin/bash
# Debian libclang for pg_query bindgen. cap-drop ALL cannot apt-get.
# Origin: a332 linux/libclang-extract.sh. Package selection stays the Rust tool.
# Changes: a346 cargo paths and evidence paths. No Python.
set -euo pipefail
ROOT=/cargo-home/a346-clang-root
DEBS=/cargo-home/a346-debs
LIST=/cargo-home/a346-Packages
OUT=/evidence/linux/execute
mkdir -p "$ROOT" "$DEBS" "$OUT" /cargo-home/bin
if [ ! -s "$LIST" ]; then
  curl -fsSL http://deb.debian.org/debian/dists/trixie/main/binary-arm64/Packages.gz | gzip -dc > "$LIST"
fi
dpkg-query -W -f '${Package}\n' > "$OUT/libclang-installed.txt"
/cargo-home/bin/select-debs "$LIST" "$OUT/libclang-installed.txt" "$OUT/libclang-debs.txt"
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
done < "$OUT/libclang-debs.txt"
libdir=$(find "$ROOT/usr/lib" \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
test -n "$libdir"
dirname "$libdir" > "$OUT/libclang-dir.txt"
echo "libclang_extracted"
