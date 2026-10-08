#!/bin/bash
# Debian libclang for pg_query bindgen. cap-drop ALL cannot apt-get.
# Extract onto this task's private cargo volume. Package selection and
# checksums are the reviewed Rust tools. No Python.
# Origin: deploy/trusted-demo/linux/extract-libclang.sh
# which records a274 extract-libclang.sh
# 982f18e430e6b8136a4cb30cb7e0711015318d0b40c0939475bfae8142e55ed4
# Change: cargo paths are /cargo/a371-* for this UUID only.
set -euo pipefail
ROOT=/cargo/a371-clang-root
DEBS=/cargo/a371-debs
LIST=/cargo/a371-Packages
PHASE=${OO_A371_PHASE:-/evidence/linux/proof/phases}
mkdir -p "$ROOT" "$DEBS" "$PHASE" /cargo/a371-bin
if [ ! -s "$LIST" ]; then
  curl -fsSL http://deb.debian.org/debian/dists/trixie/main/binary-arm64/Packages.gz | gzip -dc > "$LIST"
fi
dpkg-query -W -f '${Package}\n' > "$PHASE/libclang-installed.txt"
/cargo/a371-bin/select-debs "$LIST" "$PHASE/libclang-installed.txt" "$PHASE/libclang-debs.txt"
while read -r name filename digest; do
  [ -n "$name" ] || continue
  deb="${DEBS}/${name}.deb"
  if [ ! -s "$deb" ]; then
    curl -fsSL "http://deb.debian.org/debian/${filename}" -o "$deb"
  fi
  got=$(/cargo/a371-bin/sha256 file "$deb" | awk '{print $1}')
  if [ "$got" != "$digest" ]; then
    echo "checksum mismatch for $name want=$digest got=$got" >&2
    exit 1
  fi
  dpkg-deb -x "$deb" "$ROOT"
done < "$PHASE/libclang-debs.txt"
libdir=$(find "$ROOT/usr/lib" \( -name 'libclang.so' -o -name 'libclang-*.so' \) -print -quit)
test -n "$libdir"
dirname "$libdir" > "$PHASE/libclang-dir.txt"
echo "libclang_extracted"
