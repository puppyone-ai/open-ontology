#!/bin/bash
# Install libclang for bindgen without apt. cap-drop ALL cannot chown the
# _apt sandbox, so the debs are fetched and extracted under /tmp.
# Package set matches the approved gateway builds. Checksums come from the
# Debian Packages file fetched in this container.
set -euo pipefail
ROOT=/tmp/clang-root
LIST=/tmp/Packages
mkdir -p "$ROOT" /tmp/debs /evidence/linux
if [ ! -s "$LIST" ]; then
  curl -fsSL http://deb.debian.org/debian/dists/trixie/main/binary-arm64/Packages.gz | gzip -dc > "$LIST"
fi
python3 - "$LIST" <<'PY'
import pathlib, subprocess, sys
text = pathlib.Path(sys.argv[1]).read_text(errors="replace")
packages = {}
for stanza in text.split("\n\n"):
    fields = {}
    key = None
    for line in stanza.splitlines():
        if not line:
            continue
        if line.startswith(" ") and key:
            fields[key] += " " + line.strip()
            continue
        if ":" not in line:
            continue
        name, value = line.split(":", 1)
        key = name.strip()
        fields[key] = value.strip()
    if "Package" in fields:
        packages[fields["Package"]] = fields

installed = set(subprocess.check_output(["dpkg-query", "-W", "-f", "${Package}\n"], text=True).split())
want = ["libclang-19-dev", "libclang1-19", "pkgconf"]
queue = list(want)
seen = set(installed)
selected = []
while queue:
    name = queue.pop(0)
    if name in seen:
        continue
    fields = packages.get(name)
    if fields is None:
        raise SystemExit(f"missing package metadata: {name}")
    seen.add(name)
    selected.append(fields)
    raw = fields.get("Depends", "") + ", " + fields.get("Pre-Depends", "")
    for part in raw.split(","):
        part = part.strip()
        if not part:
            continue
        alt = part.split("|")[0].strip()
        alt = alt.split("(", 1)[0].strip()
        alt = alt.split(":", 1)[0].strip()
        if alt and alt not in seen:
            queue.append(alt)
out = pathlib.Path("/evidence/linux/libclang-debs.txt")
lines = []
for fields in selected:
    lines.append(f"{fields['Package']} {fields['Filename']} {fields['SHA256']}")
out.write_text("\n".join(lines) + "\n")
print(f"selected={len(selected)}")
PY

while read -r name filename digest; do
  deb="/tmp/debs/${name}.deb"
  if [ ! -s "$deb" ]; then
    curl -fsSL "http://deb.debian.org/debian/${filename}" -o "$deb"
  fi
  got=$(sha256sum "$deb" | awk '{print $1}')
  if [ "$got" != "$digest" ]; then
    echo "checksum mismatch for $name" >&2
    exit 1
  fi
  dpkg-deb -x "$deb" "$ROOT"
done < /evidence/linux/libclang-debs.txt

libdir=$(find "$ROOT/usr/lib" \( -name 'libclang.so' -o -name 'libclang-*.so' \) | head -1)
test -n "$libdir"
dirname "$libdir" > /evidence/linux/libclang-dir.txt
echo "libclang_extracted"
