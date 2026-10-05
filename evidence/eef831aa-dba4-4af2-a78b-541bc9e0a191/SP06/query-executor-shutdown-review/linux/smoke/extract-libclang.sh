#!/bin/bash
# Extract a private libclang tree for bindgen. No apt. No extra capabilities.
set -euo pipefail
ROOT=/cargo-home/clang-root
LIST=/tmp/Packages
mkdir -p "$ROOT" /tmp/debs /evidence/linux/smoke

fetch() {
  local url=$1 dest=$2 attempt
  for attempt in 1 2 3 4 5; do
    if curl -fsSL --retry 2 --retry-all-errors --retry-delay 1 --connect-timeout 20 --max-time 180 "$url" -o "$dest"; then
      return 0
    fi
    echo "curl attempt ${attempt} failed" >&2
    rm -f "$dest"
    sleep 1
  done
  return 52
}

fetch_deb() {
  local path=$1 dest=$2
  if fetch "https://deb.debian.org/debian/${path}" "$dest"; then
    return 0
  fi
  fetch "http://deb.debian.org/debian/${path}" "$dest"
}

if [ ! -s "$LIST" ]; then
  fetch_deb "dists/trixie/main/binary-arm64/Packages.gz" /tmp/Packages.gz
  gzip -dc /tmp/Packages.gz > "$LIST"
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
want = ["libclang-19-dev", "libclang1-19", "libz3-4", "pkgconf"]
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
        alt = part.split("|")[0].strip().split("(", 1)[0].strip().split(":", 1)[0].strip()
        if alt and alt not in seen:
            queue.append(alt)
lines = [f"{fields['Package']} {fields['Filename']} {fields['SHA256']}" for fields in selected]
path = pathlib.Path("/evidence/linux/smoke/libclang-debs.txt")
path.write_text("\n".join(lines) + "\n")
print(f"selected={len(selected)}")
PY
while read -r name filename digest; do
  deb="/tmp/debs/${name}.deb"
  if [ ! -s "$deb" ]; then
    fetch_deb "$filename" "$deb"
  fi
  got=$(/cargo-home/bin/sha256 file "$deb" | awk '{print $1}')
  if [ "$got" != "$digest" ]; then
    echo "checksum mismatch for $name" >&2
    exit 1
  fi
  dpkg-deb -x "$deb" "$ROOT"
done < /evidence/linux/smoke/libclang-debs.txt
so=$(find "$ROOT/usr/lib" -name 'libclang.so' -print -quit || true)
if [ -z "$so" ]; then
  echo "libclang.so not found" >&2
  find "$ROOT/usr/lib" -name 'libclang*' -print >&2 || true
  exit 1
fi
dirname "$so" > "$ROOT/libclang-dir.txt"
echo "libclang_extracted"
