#!/bin/bash
# Debian libclang for pg_query bindgen. cap-drop ALL cannot apt-get.
# Extract onto the private cargo volume. /tmp is noexec, so nothing is run from there.
# Package selection and checksums are the Rust tools. No Python.
# Origin: a218 extract-libclang.sh 0da7e0b4ce11be46868121dbf41b3acf48a747f3
# Change: cargo paths are a227-private. The package list and checksum check are unchanged.
set -euo pipefail
ROOT=/cargo-home/a227-clang-root
DEBS=/cargo-home/a227-debs
LIST=/cargo-home/a227-Packages
mkdir -p "$ROOT" "$DEBS" /evidence/linux/smoke
if [ ! -s "$LIST" ]; then
  curl -fsSL http://deb.debian.org/debian/dists/trixie/main/binary-arm64/Packages.gz | gzip -dc > "$LIST"
fi
dpkg-query -W -f '${Package}\n' > /evidence/linux/smoke/libclang-installed.txt
/cargo-home/bin/select-debs "$LIST" /evidence/linux/smoke/libclang-installed.txt /evidence/linux/smoke/libclang-debs.txt
while read -r name filename digest; do
  [ -n "$name" ] || continue
  deb="${DEBS}/${name}.deb"
  if [ ! -s "$deb" ]; then
    curl -fsSL "http://deb.debian.org/debian/${filename}" -o "$deb"
  fi
  got=$(/cargo-home/bin/sha256 file "$deb" | awk '{print $1}')
  if [ "$got" != "$digest" ]; then
    echo "checksum mismatch for $name want=$digest got=$got" >&2
    exit 1
  fi
  dpkg-deb -x "$deb" "$ROOT"
done < /evidence/linux/smoke/libclang-debs.txt
libdir=$(find "$ROOT/usr/lib" \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
test -n "$libdir"
dirname "$libdir" > /evidence/linux/smoke/libclang-dir.txt
echo "libclang_extracted"
