#!/bin/bash
# Debian libclang for pg_query bindgen. cap-drop ALL cannot apt-get.
# Extract onto the private cargo volume. /tmp is noexec, so nothing is run from there.
# Package selection and checksums are the Rust tools. No Python.
# Origin: a274 extract-libclang.sh sha256 982f18e430e6b8136a4cb30cb7e0711015318d0b40c0939475bfae8142e55ed4
# which records a200 extract dc0d980692091bec55d4fbc3eb12a0eda9c5aaa4b9f7d5c6eab995b2a726690d.
# Change: a317 cargo paths and the chain evidence directory.
set -eu
ROOT=/cargo-home/a317-clang-root
DEBS=/cargo-home/a317-debs
LIST=/cargo-home/a317-Packages
mkdir -p "$ROOT" "$DEBS" /evidence/linux/chain
if [ ! -s "$LIST" ]; then
  curl -fsSL http://deb.debian.org/debian/dists/trixie/main/binary-arm64/Packages.gz | gzip -dc > "$LIST"
fi
dpkg-query -W -f '${Package}\n' > /evidence/linux/chain/libclang-installed.txt
/cargo-home/bin/select-debs "$LIST" /evidence/linux/chain/libclang-installed.txt /evidence/linux/chain/libclang-debs.txt
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
done < /evidence/linux/chain/libclang-debs.txt
libdir=$(find "$ROOT/usr/lib" \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
test -n "$libdir"
dirname "$libdir" > /evidence/linux/chain/libclang-dir.txt
echo "libclang_extracted"
