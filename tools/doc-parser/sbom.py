#!/usr/bin/env python3
"""SBOM of the sidecar image, generated from inside the image (stdlib only).

    docker run --rm --entrypoint python -v $PWD/sbom.py:/sbom.py:ro IMAGE /sbom.py > sbom.json

CycloneDX 1.5 subset. Used when syft is not installed (90 §3 allows pip-licenses
style evidence from inside the image). Components:
  * every Python distribution (name, version, declared license),
  * native libraries bundled in wheels (`*.libs/`, `*.so*`) matched against a
    table of known licenses, unknown ones flagged `NOASSERTION`,
  * Debian packages of the base image (reported under `oo:scope=base-os`),
  * the model files from models.lock.json.
"""
import json
import os
import re
import sys
from importlib import metadata
from pathlib import Path

KNOWN_NATIVE = [
    # (filename regex, name, license expression)
    (r"^libgomp", "libgomp (GCC OpenMP runtime)", "GPL-3.0-or-later WITH GCC-exception-3.1"),
    (r"^libgfortran", "libgfortran (GCC Fortran runtime)", "GPL-3.0-or-later WITH GCC-exception-3.1"),
    (r"^libquadmath", "libquadmath (GCC)", "LGPL-2.1-or-later"),
    (r"^libstdc\+\+", "libstdc++ (GCC)", "GPL-3.0-or-later WITH GCC-exception-3.1"),
    (r"^libgcc_s", "libgcc_s (GCC)", "GPL-3.0-or-later WITH GCC-exception-3.1"),
    (r"^libscipy_openblas|^libopenblas", "OpenBLAS", "BSD-3-Clause"),
    (r"^libfreetype", "FreeType", "FTL OR GPL-2.0-only"),
    (r"^libharfbuzz", "HarfBuzz", "MIT"),
    (r"^libpng", "libpng", "libpng-2.0"),
    (r"^libjpeg", "libjpeg-turbo", "BSD-3-Clause AND IJG"),
    (r"^libtiff", "libtiff", "libtiff"),
    (r"^libwebp|^libsharpyuv", "libwebp", "BSD-3-Clause"),
    (r"^libopenjp2", "OpenJPEG", "BSD-2-Clause"),
    (r"^liblcms2", "Little CMS", "MIT"),
    (r"^libxcb|^libXau", "libxcb", "MIT"),
    (r"^libbrotli", "Brotli", "MIT"),
    (r"^libz\b|^libz-", "zlib", "Zlib"),
    (r"^libavif|^libdav1d|^libaom|^librav1e|^libSvtAv1", "AV1 codec library (Pillow)", "BSD-2-Clause"),
    (r"^libspatialindex", "libspatialindex (rtree)", "MIT"),
    (r"^libxml2|^libxslt|^libexslt", "libxml2/libxslt (lxml)", "MIT"),
    (r"^libiconv|^libcharset", "libiconv (lxml)", "LGPL-2.1-or-later"),
    (r"^libicu", "ICU", "Unicode-3.0"),
    (r"^libpdfium|^pdfium", "PDFium (pypdfium2)", "BSD-3-Clause AND Apache-2.0"),
    (r"^libavcodec|^libavformat|^libavutil|^libswscale|^libswresample", "FFmpeg", "LGPL-2.1-or-later"),
    (r"^libopencv|^cv2", "OpenCV", "Apache-2.0"),
    (r"^liblzma", "liblzma (xz)", "0BSD"),
    (r"^libzstd", "zstd", "BSD-3-Clause OR GPL-2.0-only"),
    (r"^ld-linux|^libc\.so|^libm\.so|^libpthread", "glibc (vendored by auditwheel)", "LGPL-2.1-or-later"),
    (r"^libtorch|^libc10|^libshm|^libtorch_", "PyTorch native", "BSD-3-Clause"),
]


def py_components():
    out = []
    for d in sorted(metadata.distributions(), key=lambda x: (x.metadata["Name"] or "").lower()):
        md = d.metadata
        name = md["Name"]
        if not name:
            continue
        lic = (md.get("License-Expression") or "").strip()
        src = "License-Expression"
        if not lic:
            raw = (md.get("License") or "").strip()
            if raw and "\n" not in raw and len(raw) <= 120:
                lic, src = raw, "License"
            elif raw:
                lic, src = raw.splitlines()[0][:120] + " (text)", "License"
        if not lic:
            cls = [c.split("::")[-1].strip() for c in md.get_all("Classifier", []) if c.startswith("License ::")]
            if cls:
                lic, src = " AND ".join(cls), "Classifier"
        out.append({
            "type": "library", "name": name, "version": md["Version"],
            "purl": f"pkg:pypi/{name.lower().replace('_', '-')}@{md['Version']}",
            "licenses": [{"expression": lic}] if lic else [],
            "properties": [{"name": "oo:license_source", "value": src if lic else "none"},
                           {"name": "oo:scope", "value": "python"}],
        })
    return out


def native_components(site):
    seen = {}
    for p in site.rglob("*"):
        if not (p.is_file() or p.is_symlink()):
            continue
        n = p.name
        in_libs = any(part.endswith(".libs") or part == ".libs" for part in p.parts)
        if not (in_libs or ".so" in n):
            continue
        base = re.sub(r"-[0-9a-f]{8}(?=\.so)", "", n)
        for rx, label, lic in KNOWN_NATIVE:
            if re.search(rx, base):
                seen.setdefault(label, {"license": lic, "files": set()})["files"].add(base)
                break
        else:
            if in_libs:
                seen.setdefault(f"unidentified:{base.split('.so')[0]}", {"license": "NOASSERTION", "files": set()})["files"].add(base)
    out = []
    for label, v in sorted(seen.items()):
        out.append({
            "type": "library", "name": label, "version": "bundled",
            "licenses": [{"expression": v["license"]}],
            "properties": [{"name": "oo:scope", "value": "native-in-wheel"},
                           {"name": "oo:files", "value": ",".join(sorted(v["files"]))[:400]}],
        })
    return out


def deb_components():
    out = []
    status = Path("/var/lib/dpkg/status")
    if not status.exists():
        return out
    for block in status.read_text(errors="replace").split("\n\n"):
        f = dict(re.findall(r"^([A-Za-z-]+): (.*)$", block, re.M))
        if "Package" not in f or "installed" not in f.get("Status", ""):
            continue
        out.append({
            "type": "library", "name": f["Package"], "version": f.get("Version", ""),
            "purl": f"pkg:deb/debian/{f['Package']}@{f.get('Version', '')}",
            "licenses": [],
            "properties": [{"name": "oo:scope", "value": "base-os"},
                           {"name": "oo:license_source", "value": f"/usr/share/doc/{f['Package']}/copyright"}],
        })
    return out


def model_components(lock_path):
    try:
        lock = json.loads(Path(lock_path).read_text())
    except OSError:
        return []
    lic = {s["local_dir"]: s.get("license", "NOASSERTION") for s in lock.get("sources", [])}
    out = []
    for f in lock.get("files", []):
        d = f["name"].split("/", 1)[0]
        out.append({
            "type": "machine-learning-model", "name": f["name"], "version": "pinned",
            "hashes": [{"alg": "SHA-256", "content": f["sha256"]}],
            "licenses": [{"expression": lic.get(d, "NOASSERTION")}],
            "properties": [{"name": "oo:scope", "value": "model"}],
        })
    return out


def main():
    site = next(Path(p) for p in sys.path if p.endswith("site-packages"))
    comps = py_components() + native_components(site) + deb_components() + model_components(
        os.environ.get("OO_PARSER_MODELS_LOCK", "/opt/app/models.lock.json"))
    doc = {
        "bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
        "metadata": {
            "component": {"type": "container", "name": "oo-docs-parser", "version": os.environ.get("OO_IMAGE_REF", "dev")},
            "tools": [{"name": "tools/doc-parser/sbom.py", "version": "1"}],
            "properties": [{"name": "oo:python", "value": sys.version.split()[0]}],
        },
        "components": comps,
    }
    json.dump(doc, sys.stdout, ensure_ascii=False, indent=1, sort_keys=True)
    print()


if __name__ == "__main__":
    main()
