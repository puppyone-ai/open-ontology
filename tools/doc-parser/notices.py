#!/usr/bin/env python3
"""THIRD_PARTY_NOTICES for the doc-parser image, generated from its SBOM (90 §3, §6).

    python3 -B tools/doc-parser/notices.py SBOM.json > tools/doc-parser/THIRD_PARTY_NOTICES.md
    python3 -B tools/doc-parser/notices.py SBOM.json --check tools/doc-parser/THIRD_PARTY_NOTICES.md

Stdlib only, deterministic (no timestamps), no network. The SBOM comes from
`sbom.py` run inside the image. License texts come from `license-texts/` next
to this file (Debian common-licenses copies, the GCC runtime exception from the
numpy wheel, the CDLA-Permissive-2.0 text). The image carries the result at
/usr/share/licenses/oo-docs-parser/THIRD_PARTY_NOTICES.md.

Obligations are never dropped silently: an LGPL, MPL or GPL-with-exception
component that has no entry in SOURCES makes the generator exit 2, so a new
weak-copyleft component cannot reach the image without a source location here.
"""
import json
import os
import re
import sys
from pathlib import Path

import sbom_gate

HERE = Path(__file__).resolve().parent
TEXTS = HERE / "license-texts"
IMAGE_NAME = os.environ.get("OO_NOTICES_IMAGE", "oo-docs-parser")  # ocr-parser and media-parser set their own
IMAGE_PATH = f"/usr/share/licenses/{IMAGE_NAME}/THIRD_PARTY_NOTICES.md"

# Where the corresponding source of each weak-copyleft component lives, and how
# it is shipped. Keys are SBOM component names.
SOURCES = {
    "certifi": {
        "kind": "mpl",
        "url": "https://github.com/certifi/python-certifi",
        "how": "Unmodified PyPI wheel. The Covered Software files are the Python source and the CA bundle data, which stay in the image as shipped, so the source form is the file itself.",
    },
    "tqdm": {
        "kind": "mpl",
        "url": "https://github.com/tqdm/tqdm",
        "how": "Unmodified PyPI wheel. The MPL-2.0 files are plain Python source and stay in the image as shipped.",
    },
    "glibc (vendored by auditwheel)": {
        "kind": "lgpl",
        "url": "https://ftp.gnu.org/gnu/glibc/",
        "how": "The dynamic loader that auditwheel vendored into a manylinux wheel (a separate shared object, never linked into our code; replaceable by deleting it so the system loader is used, or by dropping in another build with the same soname).",
    },
    "libgfortran (GCC Fortran runtime)": {
        "kind": "gcc-runtime",
        "url": "https://ftp.gnu.org/gnu/gcc/",
        "how": "Unmodified GCC runtime library shipped inside the numpy and scipy wheels as a shared object.",
    },
    "libgomp (GCC OpenMP runtime)": {
        "kind": "gcc-runtime",
        "url": "https://ftp.gnu.org/gnu/gcc/",
        "how": "Unmodified GCC OpenMP runtime shipped inside the torch and numpy wheels as a shared object.",
    },
    "libquadmath (GCC)": {
        "kind": "lgpl",
        "url": "https://ftp.gnu.org/gnu/gcc/",
        "how": "Unmodified GCC runtime library shipped inside the numpy and scipy wheels as a shared object.",
    },
    "libstdc++ (GCC)": {
        "kind": "gcc-runtime",
        "url": "https://ftp.gnu.org/gnu/gcc/",
        "how": "Unmodified GCC runtime library shipped inside a wheel as a shared object.",
    },
    "libgcc_s (GCC)": {
        "kind": "gcc-runtime",
        "url": "https://ftp.gnu.org/gnu/gcc/",
        "how": "Unmodified GCC runtime library shipped inside a wheel as a shared object.",
    },
    "ffmpeg-lgpl-own-build": {
        "kind": "lgpl",
        "url": "https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz",
        "how": "Built from the unmodified source tarball (sha256 pinned in tools/media-parser/Dockerfile) with --disable-gpl --disable-nonfree --disable-everything and a decode-only allow-list (no H.264, HEVC or AAC decoder); dynamically linked shared objects in /opt/ffmpeg/lib, replaceable. The LGPL text, the configure line and the source tarball are in /usr/share/licenses/ffmpeg/ (90 section 4).",
    },
    "crc32c": {
        "kind": "lgpl",
        "url": "https://github.com/ICRAR/crc32c",
        "how": "Unmodified PyPI wheel (a compiled Python extension). Pulled in by bce-python-sdk, a PaddleX dependency. Replaceable: remove the wheel or install another build of the same version.",
    },
    "cssutils": {
        "kind": "lgpl",
        "url": "https://github.com/jaraco/cssutils",
        "how": "Unmodified PyPI wheel, pure Python source kept as shipped. Pulled in by premailer (PaddleX document parsing dependency).",
    },
    "encutils": {
        "kind": "lgpl",
        "url": "https://github.com/jaraco/encutils",
        "how": "Unmodified PyPI wheel, pure Python source kept as shipped. Dependency of cssutils.",
    },
    "python-bidi": {
        "kind": "lgpl",
        "url": "https://github.com/MeirKriheli/python-bidi",
        "how": "Unmodified PyPI wheel (a compiled Python extension). Pulled in by PaddleOCR.",
    },
    "libasound (ALSA library, bundled by sherpa-onnx)": {
        "kind": "lgpl",
        "url": "https://www.alsa-project.org/wiki/Download",
        "how": "Unmodified ALSA library (libasound.so.2) vendored in the sherpa-onnx manylinux wheel, a separate shared object loaded dynamically (replaceable by another build with the same file name). It is used for microphone capture only; Super Ontology reads audio files and never opens a sound device.",
    },
    "libgeos (shapely)": {
        "kind": "lgpl",
        "url": "https://libgeos.org/usage/download/",
        "how": "Unmodified GEOS shared object vendored in the shapely wheel (a separate library loaded dynamically; replaceable by another build with the same file name).",
    },
    "libgeos_c (shapely)": {
        "kind": "lgpl",
        "url": "https://libgeos.org/usage/download/",
        "how": "Unmodified GEOS C API shared object vendored in the shapely wheel (a separate library loaded dynamically).",
    },
    "libiconv (lxml)": {
        "kind": "lgpl",
        "url": "https://ftp.gnu.org/gnu/libiconv/",
        "how": "Shared object shipped inside a wheel.",
    },
}

CANONICAL = {
    "Apache-2.0": "Apache-2.0.txt",
    "LGPL-2.1": "LGPL-2.1.txt",
    "LGPL-3.0": "LGPL-3.0.txt",
    "MPL-2.0": "MPL-2.0.txt",
    "GPL-3.0": "GPL-3.txt",
    "GCC-exception-3.1": "GCC-exception-3.1.txt",
    "CDLA-Permissive-2.0": "CDLA-Permissive-2.0.txt",
}


class NoticeError(Exception):
    pass


def prop(c, name):
    return next((p["value"] for p in c.get("properties", []) if p["name"] == name), "")


def lic_of(c):
    return " AND ".join(
        l.get("expression") or l.get("license", {}).get("id", "") for l in c.get("licenses", [])
    ) or "NOASSERTION"


def needs_source(lic):
    """Weak-copyleft obligations apply unless the expression offers a permissive
    alternative (`A OR B`: we take the best one, as the SBOM gate does)."""
    return sbom_gate.classify(lic) != "pass"


def esc(s):
    return str(s).replace("|", "\\|").replace("\n", " ")


def text(name):
    return (TEXTS / CANONICAL[name]).read_text(encoding="utf-8").rstrip("\n")


def generate(sbom):
    comps = sbom["components"]
    by_scope = {}
    for c in comps:
        by_scope.setdefault(prop(c, "oo:scope"), []).append(c)
    for v in by_scope.values():
        v.sort(key=lambda c: (c["name"].lower(), c.get("version", "")))
    image = sbom.get("metadata", {}).get("component", {})
    out = []
    w = out.append
    w(f"# Third-party notices: {IMAGE_NAME} image")
    w("")
    w("This file is generated from the image SBOM by `tools/doc-parser/notices.py`; do not edit it by hand. "
      "It ships in the image at `" + IMAGE_PATH + "`. Every Python package keeps its own license and notice files "
      "unmodified under `/opt/venv/lib/python3.13/site-packages/<distribution>.dist-info/`; Debian packages of the "
      "base image keep theirs under `/usr/share/doc/<package>/copyright`. This file adds the inventory, the source "
      "offer and the notices that the weak-copyleft licenses require.")
    w("")
    w(f"Image: `{image.get('name', IMAGE_NAME)}` ({image.get('version', 'dev')}). "
      f"Components: {len(by_scope.get('python', []))} Python distributions, "
      f"{len(by_scope.get('native-in-wheel', []))} native libraries inside wheels, "
      f"{len(by_scope.get('model', []))} model files, {len(by_scope.get('base-os', []))} base-image packages.")
    w("")

    # --- obligations
    needed = []
    for scope in ("python", "native-in-wheel", "own-build"):
        for c in by_scope.get(scope, []):
            if needs_source(lic_of(c)):
                needed.append(c)
    missing = [c["name"] for c in needed if c["name"] not in SOURCES]
    if missing:
        raise NoticeError("no source entry in notices.py SOURCES for: " + ", ".join(sorted(set(missing))))
    kinds = {SOURCES[c["name"]]["kind"] for c in needed}

    w("## 1. Source offer for weak-copyleft components (LGPL, GPL with runtime exception, MPL)")
    w("")
    w("The components below are licensed under the GNU LGPL, under the GPL with the GCC Runtime Library Exception, "
      "or under the MPL-2.0. They are shipped unmodified; the native ones are separate shared objects that are "
      "dynamically loaded, none is linked into or modified by our code, and each can be replaced in the image by "
      "another build with the same file name. "
      "We provide, for every such component and for as long as we distribute this image, the complete corresponding "
      "source of the exact version shipped: from the upstream location named in the table, and on request as a "
      "release artifact of the repository that publishes this image (the source tarball and the build line, "
      "per spec 90 section 4). Our end-user terms must not forbid reverse engineering for the purpose of debugging "
      "modifications to these libraries (spec 90 section 4).")
    w("")
    w("| Component | Version | License | Upstream source | How it is shipped |")
    w("|---|---|---|---|---|")
    for c in needed:
        s = SOURCES[c["name"]]
        w(f"| {esc(c['name'])} | {esc(c.get('version', ''))} | {esc(lic_of(c))} | {s['url']} | {esc(s['how'])} |")
    if not needed:
        w("| (none in this SBOM) | | | | |")
    w("")
    w("## 2. MPL-2.0 notices")
    w("")
    mpl = [c for c in needed if SOURCES[c["name"]]["kind"] == "mpl"]
    if mpl:
        w("The following components contain files under the Mozilla Public License 2.0. They are shipped unmodified, "
          "in the source form in which upstream publishes them, so the Source Code Form is available from the image "
          "itself and from the upstream address below. The MPL-2.0 text is in section 6.")
        w("")
        for c in mpl:
            w(f"- {c['name']} {c.get('version', '')}: {SOURCES[c['name']]['url']}")
    else:
        w("No MPL-2.0 component in this SBOM.")
    w("")
    w("## 3. OpenCV (built without FFmpeg)")
    w("")
    cv = [c for c in by_scope.get("python", []) if re.match(r"opencv", c["name"], re.I)]
    if cv:
        c = cv[0]
        w(f"{c['name']} {c.get('version', '')} (Apache-2.0) is built from source in the Dockerfile stage `opencv` "
          "from the pinned PyPI source distribution (URL and sha256 in `tools/doc-parser/Dockerfile`) with FFmpeg, "
          "GStreamer, Video4Linux and all video modules off and only the core and imgproc modules built "
          f"(attestation in the SBOM: `oo:opencv_ffmpeg={prop(c, 'oo:opencv_ffmpeg') or 'unknown'}`). "
          "The image contains no libavcodec, libavformat, libavutil, libswscale or libswresample (D65). "
          "OpenCV's own license and notice files are in its dist-info directory.")
    else:
        w("No OpenCV in this SBOM.")
    w("")
    w("## 4. Model weights")
    w("")
    w("| File | License as declared by the publisher | SHA-256 |")
    w("|---|---|---|")
    for c in by_scope.get("model", []):
        h = next((x["content"] for x in c.get("hashes", []) if x.get("alg") == "SHA-256"), "")
        w(f"| {esc(c['name'])} | {esc(lic_of(c))} | `{h}` |")
    tf = [c for c in by_scope.get("model", []) if "docling-models" in c["name"]]
    w("")
    if tf:
        w("The TableFormer weights (Hugging Face `docling-project/docling-models`) carry two license tags, "
          "CDLA-Permissive-2.0 and Apache-2.0, and the repository ships no license file; both texts are reproduced "
          "in section 6. Which of the two governs is an open question for counsel (spec 90 section 9, L5). "
          "Both are permissive and ask for notice retention only.")
    w("The layout model (`docling-project/docling-layout-heron`) is Apache-2.0; its text is in section 6.")
    w("")
    w("## 5. Inventory")
    w("")
    w("| Component | Version | License | Kind |")
    w("|---|---|---|---|")
    for scope, label in (("python", "Python distribution"), ("native-in-wheel", "native library in a wheel")):
        for c in by_scope.get(scope, []):
            w(f"| {esc(c['name'])} | {esc(c.get('version', ''))} | {esc(lic_of(c))} | {label} |")
    w("")
    choices = [c for c in by_scope.get("python", []) + by_scope.get("native-in-wheel", []) if " OR " in lic_of(c)]
    if choices:
        w("Where a component is offered under a choice of licenses (`OR`) we use the permissive alternative: "
          + "; ".join(f"{c['name']} ({lic_of(c)})" for c in choices) + ".")
        w("")
    if any(c["name"] == "FreeType" for c in by_scope.get("native-in-wheel", [])):
        w("Portions of this software are copyright (c) The FreeType Project (www.freetype.org). All rights reserved. "
          "FreeType is used under the FreeType License (FTL); the credit line is the one the FTL asks for.")
        w("")
    w("### Base image packages")
    w("")
    w("Debian packages of the pinned `python` base image; license text of each in `/usr/share/doc/<package>/copyright`. "
      "Several are LGPL or GPL system libraries (libc6, libgcc-s1, libstdc++6, ...) that come with Debian and are "
      "not modified; Debian offers their source at https://sources.debian.org/ and `apt-get source <package>`.")
    w("")
    base = ", ".join(f"{c['name']} {c.get('version', '')}".strip() for c in by_scope.get("base-os", []))
    w(base or "(none)")
    w("")
    w("## 6. License texts")
    w("")
    all_lic = " ".join(lic_of(c) for c in comps if prop(c, "oo:scope") != "base-os")
    appendix = []
    if re.search(r"Apache-2\.0|Apache", all_lic):
        appendix.append(("Apache License 2.0", "Apache-2.0"))
    if "mpl" in kinds or re.search(r"\bMPL", all_lic):
        appendix.append(("Mozilla Public License 2.0", "MPL-2.0"))
    if "lgpl" in kinds or re.search(r"\bLGPL", all_lic):
        appendix.append(("GNU Lesser General Public License 2.1", "LGPL-2.1"))
    if re.search(r"LGPL-3|LGPLv3|LGPL-3\.0|\(LGPL\)", all_lic) or any(
        "LGPL-3" in lic_of(c) for c in comps if prop(c, "oo:scope") != "base-os"
    ):
        appendix.append(("GNU Lesser General Public License 3.0 (cssutils, encutils)", "LGPL-3.0"))
    if "gcc-runtime" in kinds or re.search(r"GCC-exception", all_lic):
        appendix.append(("GNU General Public License 3.0 (applies to the GCC runtime libraries together with the exception below)", "GPL-3.0"))
        appendix.append(("GCC Runtime Library Exception 3.1", "GCC-exception-3.1"))
    if tf:
        appendix.append(("Community Data License Agreement Permissive 2.0 (TableFormer weights)", "CDLA-Permissive-2.0"))
    for title, key in appendix:
        w(f"### {title}")
        w("")
        w("```text")
        w(text(key))
        w("```")
        w("")
    return "\n".join(out).rstrip("\n") + "\n"


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    sbom = json.loads(Path(args[0]).read_text())
    try:
        doc = generate(sbom)
    except NoticeError as e:
        print(f"notices: {e}", file=sys.stderr)
        sys.exit(2)
    if "--check" in sys.argv:
        target = Path(sys.argv[sys.argv.index("--check") + 1])
        if target.read_text(encoding="utf-8") != doc:
            print(f"notices: {target} is stale; regenerate it from the SBOM", file=sys.stderr)
            sys.exit(1)
        print("notices: up to date")
        return
    sys.stdout.write(doc)


if __name__ == "__main__":
    main()
