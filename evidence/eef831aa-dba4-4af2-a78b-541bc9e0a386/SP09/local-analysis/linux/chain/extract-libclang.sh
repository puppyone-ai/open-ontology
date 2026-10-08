#!/bin/bash
# Re-extract the recorded Debian libclang set onto this task's cargo volume.
# Origin: a333 extract-libclang.sh dca0a1b8452ca3c0a4f9ae9c2529d53687d2543ca3fec0077bdcab16debe7076
# which records a327 live-3 e8b104c9a7af9db4c183fcbe76ccb0c872f929d10938763b77dfe5bd4a8929a0
# Changes: a344 cargo paths and this review's deb list.
set -euo pipefail
ROOT=/cargo-home/a386-clang-root
DEBS=/cargo-home/a386-debs
LIST=/evidence/linux/libclang-debs.txt
OUT=/evidence/linux/chain
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
