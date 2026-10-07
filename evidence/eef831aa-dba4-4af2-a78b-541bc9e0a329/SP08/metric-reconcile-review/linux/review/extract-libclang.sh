#!/bin/bash
# Debian libclang for pg_query bindgen. cap-drop ALL cannot apt-get.
# Extract onto the private cargo volume. /tmp is noexec, so nothing is run from there.
# Package selection and checksums are the Rust tools. No Python.
# Origin a200 extract-libclang.sh dc0d980692091bec55d4fbc3eb12a0eda9c5aaa4b9f7d5c6eab995b2a726690d
# via a316 extract-libclang.sh 025174865bb7d489b7994ffa8301c0dc56283d872e2b366bef933b69af0aa1de.
# Change: cargo-home prefix a316 -> a329. Evidence path stays linux/smoke.
set -euo pipefail
ROOT=/cargo-home/a329-clang-root
DEBS=/cargo-home/a329-debs
LIST=/cargo-home/a329-Packages
mkdir -p "$ROOT" "$DEBS" /evidence/linux/smoke
if [ ! -s "$LIST" ]; then
  curl -fsSL http://deb.debian.org/debian/dists/trixie/main/binary-arm64/Packages.gz | gzip -dc > "$LIST"
fi
dpkg-query -W -f '${Package}\n' > /evidence/linux/smoke/libclang-installed.txt
/cargo-home/bin/sha256 file "$LIST" > /evidence/linux/smoke/packages-sha256.txt || true
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
