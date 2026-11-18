#!/usr/bin/env python3
"""CycloneDX-subset SBOM of the minimal pgvector image (deploy/docs/pgvector/Dockerfile).

    sbom_debian_pg_image.py IMAGE [--label oo.lane=...] > sbom.json
    python3 tools/doc-parser/sbom_gate.py sbom.json

Components: the dpkg database (scope base-os: listed, covered by the pinned base image review, not gated),
and what the image adds on top of the base, each with its upstream licence asserted here: the pgvector
extension (version read from vector.control) and the `gosu` binary the official base ships in /usr/local/bin.
Any other binary in /usr/local/bin or extension not owned by a dpkg package is added without a licence so the
gate reports it as unknown instead of ignoring it. Shell scripts there are the official image's entrypoint scripts (MIT).
"""
import json
import subprocess
import sys

SCRIPT = r'''
dpkg-query -W -f='D\t${Package}\t${Version}\n'
for f in /usr/share/postgresql/17/extension/*.control; do
  n=$(basename "$f" .control); v=$(sed -n "s/^default_version *= *'\(.*\)'/\1/p" "$f")
  if dpkg -S "$f" >/dev/null 2>&1; then echo "O\t$n\t$v"; else echo "E\t$n\t$v"; fi
done
for f in /usr/local/bin/*; do
  [ -e "$f" ] || continue
  if [ "$(head -c2 "$f")" = "#!" ]; then echo "S\t$(basename $f)\t-"; else echo "B\t$(basename $f)\t$($f --version 2>&1 | head -1 | tr '\t' ' ')"; fi
done
'''
# upstream licences of what the image adds; extensions that ship with the postgres package are base-os
KNOWN_ADDED = {"vector": "PostgreSQL", "gosu": "Apache-2.0", "docker-library entrypoint scripts": "MIT"}


def main():
    image = sys.argv[1]
    label = sys.argv[sys.argv.index("--label") + 1] if "--label" in sys.argv else "oo.lane=super-ontology-c8"
    out = subprocess.run(["docker", "run", "--rm", "--label", label, "--entrypoint", "sh", image, "-c", SCRIPT],
                         check=True, capture_output=True, text=True).stdout
    comps = []
    scripts = set()
    for line in out.splitlines():
        kind, _, rest = line.partition("\t")
        name, _, ver = rest.partition("\t")
        if kind == "D":
            comps.append(("base-os", name, ver, None))
        elif kind == "O":
            continue  # a contrib extension owned by a dpkg package: already in the base-os list
        elif kind == "E":
            comps.append(("native-in-image", name, ver, KNOWN_ADDED.get(name)))
        elif kind == "B":
            comps.append(("native-in-image", name, ver.split(" (")[0], KNOWN_ADDED.get(name)))
        elif kind == "S":
            scripts.add(name)
    if scripts:
        comps.append(("native-in-image", "docker-library entrypoint scripts", ", ".join(sorted(scripts)),
                      KNOWN_ADDED["docker-library entrypoint scripts"]))
    doc = {"bomFormat": "CycloneDX", "specVersion": "1.5", "metadata": {"component": {"name": image}}, "components": [
        {"type": "library", "name": n, "version": v, "licenses": [{"expression": lic}] if lic else [],
         "properties": [{"name": "oo:scope", "value": scope}]} for scope, n, v, lic in comps]}
    print(json.dumps(doc, indent=1))


if __name__ == "__main__":
    main()
