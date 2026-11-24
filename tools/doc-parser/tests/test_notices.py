import json
import subprocess
import sys
from pathlib import Path

import pytest

import notices

ROOT = Path(__file__).resolve().parents[1]


def comp(name, lic, scope="python", version="1", extra=None):
    props = [{"name": "oo:scope", "value": scope}] + (extra or [])
    return {"type": "library", "name": name, "version": version,
            "licenses": [{"expression": lic}] if lic else [], "properties": props}


def sbom(*comps):
    return {"metadata": {"component": {"name": "oo-docs-parser", "version": "t"}}, "components": list(comps)}


def test_weak_copyleft_without_a_source_entry_is_refused():
    with pytest.raises(notices.NoticeError, match="mystery-lib"):
        notices.generate(sbom(comp("mystery-lib", "LGPL-2.1-or-later", scope="native-in-wheel")))


def test_source_offer_mpl_and_texts_follow_the_sbom():
    doc = notices.generate(sbom(
        comp("certifi", "MPL-2.0", version="2026.10.23"),
        comp("glibc (vendored by auditwheel)", "LGPL-2.1-or-later", scope="native-in-wheel", version="bundled"),
        comp("libgomp (GCC OpenMP runtime)", "GPL-3.0-or-later WITH GCC-exception-3.1", scope="native-in-wheel", version="bundled"),
        comp("opencv-python-headless", "Apache-2.0", version="5.0.0.93", extra=[{"name": "oo:opencv_ffmpeg", "value": "absent"}]),
        comp("libdeb", None, scope="base-os", version="9"),
    ))
    assert "## 1. Source offer" in doc and "https://ftp.gnu.org/gnu/glibc/" in doc
    assert "certifi 2026.10.23: https://github.com/certifi/python-certifi" in doc
    assert "oo:opencv_ffmpeg=absent" in doc
    assert "libdeb 9" in doc
    for needle in ("Apache License", "Mozilla Public License Version 2.0", "GNU LESSER GENERAL PUBLIC LICENSE",
                   "GCC RUNTIME LIBRARY EXCEPTION"):
        assert needle in doc, needle
    assert "Community Data License" not in doc  # no TableFormer in this SBOM


def test_tableformer_brings_both_license_texts():
    model = {"type": "machine-learning-model", "name": "docling-project--docling-models/x.safetensors",
             "version": "pinned", "hashes": [{"alg": "SHA-256", "content": "ab" * 32}],
             "licenses": [{"expression": "CDLA-Permissive-2.0 AND Apache-2.0"}],
             "properties": [{"name": "oo:scope", "value": "model"}]}
    doc = notices.generate(sbom(model))
    assert "Community Data License Agreement" in doc and "Apache License" in doc and "L5" in doc


def test_generation_is_deterministic():
    s = sbom(comp("b", "MIT"), comp("a", "MIT"))
    assert notices.generate(s) == notices.generate(s)
    assert notices.generate(s).index("| a |") < notices.generate(s).index("| b |")


def committed():
    base = ROOT.parents[1] / "testing/super-ontology/evidence/L3/sbom-doc-parser.json"
    doc = ROOT / "THIRD_PARTY_NOTICES.md"
    return base, doc


def test_committed_notices_match_the_committed_sbom():
    sb, doc = committed()
    if not (sb.exists() and doc.exists()):
        pytest.skip("evidence SBOM or notices not present")
    out = subprocess.run([sys.executable, "-B", str(ROOT / "notices.py"), str(sb), "--check", str(doc)],
                         capture_output=True, text=True)
    assert out.returncode == 0, out.stderr
