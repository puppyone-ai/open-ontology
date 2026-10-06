#!/bin/bash
# Debian libclang for pg_query bindgen. cap-drop ALL cannot apt-get.
# Extract onto the private cargo volume. /tmp is noexec, so nothing is run from there.
# Package selection and checksums are the Rust tools. No Python.
# Origin: evidence/eef831aa-dba4-4af2-a78b-541bc9e0a222/SP12/query-result-adapter/linux/proof/extract-libclang.sh
# sha256 6a39526820f55f7f9508848dc8a08f571c91b2813561601db9e3cede0ef778e0
# Delta: paths are a244.
set -euo pipefail
ROOT=/cargo-home/a244-clang-root
DEBS=/cargo-home/a244-debs
LIST=/cargo-home/a244-Packages
mkdir -p "$ROOT" "$DEBS" /evidence/libclang
if [ ! -s "$LIST" ]; then
  curl -fsSL http://deb.debian.org/debian/dists/trixie/main/binary-arm64/Packages.gz | gzip -dc > "$LIST"
fi
dpkg-query -W -f '${Package}\n' > /evidence/libclang/libclang-installed.txt
/cargo-home/bin/select-debs "$LIST" /evidence/libclang/libclang-installed.txt /evidence/libclang/libclang-debs.txt
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
done < /evidence/libclang/libclang-debs.txt
libdir=$(find "$ROOT/usr/lib" \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
test -n "$libdir"
dirname "$libdir" > /evidence/libclang/libclang-dir.txt
echo "libclang_extracted"
