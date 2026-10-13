#!/bin/bash
# Debian libclang for the pg_query build script. cap-drop ALL cannot apt-get.
# Package selection is the reviewed select-debs tool. Checksums come from the
# Packages file that this run downloads. Nothing is installed into the image.
set -euo pipefail
ROOT=/cargo-home/clang-root
DEBS=/cargo-home/debs
LIST=/cargo-home/Packages
PHASE=${OO_PHASE:?}
mkdir -p "$ROOT" "$DEBS" "$PHASE"
if [ ! -s "$LIST" ]; then
  curl -fsSL --retry 5 --retry-delay 2 http://deb.debian.org/debian/dists/trixie/main/binary-arm64/Packages.gz | gzip -dc > "$LIST"
fi
dpkg-query -W -f '${Package}\n' > "$PHASE/libclang-installed.txt"
/cargo-home/bin/select-debs "$LIST" "$PHASE/libclang-installed.txt" "$PHASE/libclang-debs.txt"
while read -r name filename digest; do
  [ -n "$name" ] || continue
  deb="${DEBS}/${name}.deb"
  if [ ! -s "$deb" ]; then
    curl -fsSL --retry 5 --retry-delay 2 "http://deb.debian.org/debian/${filename}" -o "$deb.partial"
    mv "$deb.partial" "$deb"
  fi
  got=$(sha256sum "$deb" | awk '{print $1}')
  if [ "$got" != "$digest" ]; then
    echo "checksum mismatch for $name want=$digest got=$got" >&2
    exit 1
  fi
done < "$PHASE/libclang-debs.txt"
rm -rf "$ROOT"
mkdir -p "$ROOT"
while read -r name filename digest; do
  [ -n "$name" ] || continue
  dpkg-deb -x "${DEBS}/${name}.deb" "$ROOT"
done < "$PHASE/libclang-debs.txt"
libso=$(find "$ROOT/usr/lib" \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
test -n "$libso"
dirname "$libso" > "$PHASE/libclang-dir.txt"
echo "libclang_extracted"
