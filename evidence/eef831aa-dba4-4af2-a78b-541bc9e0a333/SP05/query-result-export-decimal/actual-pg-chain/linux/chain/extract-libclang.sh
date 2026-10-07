#!/bin/bash
# Re-extract the recorded Debian libclang set onto this task's cargo volume.
# Origin: a340 extract-libclang.sh, itself from a327 live-3
# sha256 e8b104c9a7af9db4c183fcbe76ccb0c872f929d10938763b77dfe5bd4a8929a0
# Changes: a333 cargo paths and this chain's deb list.
set -euo pipefail
ROOT=/cargo-home/a333-clang-root
DEBS=/cargo-home/a333-debs
LIST=/evidence/actual-pg-chain/linux/libclang-debs.txt
OUT=/evidence/actual-pg-chain/linux/chain
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
