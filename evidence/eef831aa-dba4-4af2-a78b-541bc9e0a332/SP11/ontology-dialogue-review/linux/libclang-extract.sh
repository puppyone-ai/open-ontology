#!/bin/bash
# Debian libclang for pg_query bindgen. cap-drop ALL cannot apt-get.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/smoke/extract-libclang.sh
# Changes: a332 cargo paths and evidence paths. Package selection stays the Rust tool.
set -euo pipefail
ROOT=/cargo-home/a332-clang-root
DEBS=/cargo-home/a332-debs
LIST=/cargo-home/a332-Packages
OUT=/evidence/linux/live
mkdir -p "$ROOT" "$DEBS" "$OUT"
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
