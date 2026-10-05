#!/bin/bash
# Debian libclang for pg_query bindgen. cap-drop ALL cannot apt-get.
# Extract onto this task's private cargo volume. /tmp is noexec.
# Package selection and checksums are the Rust tools. No Python.
# Origin: a200 extract-libclang.sh blob 1c23dbd6ab58cb0f42d218075fd55c0a8c4fe5ea
# with the cargo-volume directory names changed to a211.
set -euo pipefail
ROOT=/cargo-home/a211-clang-root
DEBS=/cargo-home/a211-debs
LIST=/cargo-home/a211-Packages
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
