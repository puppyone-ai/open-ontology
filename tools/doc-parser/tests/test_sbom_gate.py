import json
import subprocess
import sys
from pathlib import Path

import pytest

import sbom_gate

ROOT = Path(__file__).resolve().parents[1]


@pytest.mark.parametrize(
    "expr,want",
    [
        ("MIT", "pass"),
        ("Apache-2.0 AND MIT", "pass"),
        ("MIT OR GPL-3.0-only", "pass"),  # we may pick the permissive side
        ("BSD-3-Clause AND IJG", "pass"),
        ("GPL-3.0-or-later", "fail"),
        ("GPL-2.0-only OR AGPL-3.0-only", "fail"),
        ("AGPL-3.0-only", "fail"),
        ("BUSL-1.1", "fail"),
        ("SSPL-1.0", "fail"),
        ("Elastic-2.0", "fail"),
        ("CC-BY-NC-4.0", "fail"),
        ("LicenseRef-Proprietary", "fail"),
        ("LGPL-2.1-or-later", "review"),
        ("GPL-3.0-or-later WITH GCC-exception-3.1", "review"),
        ("MPL-2.0", "review"),
        ("MPL-2.0 AND MIT", "review"),
        ("NOASSERTION", "unknown"),
        ("", "unknown"),
    ],
)
def test_classify(expr, want):
    assert sbom_gate.classify(expr) == want


def comp(name, lic, scope="python", files=None, extra=None):
    props = [{"name": "oo:scope", "value": scope}] + (extra or [])
    if files:
        props.append({"name": "oo:files", "value": files})
    return {"type": "library", "name": name, "version": "1", "licenses": [{"expression": lic}] if lic else [], "properties": props}


def run(tmp_path, comps, reviews=None):
    p = tmp_path / "sbom.json"
    p.write_text(json.dumps({"components": comps}))
    cmd = [sys.executable, "-B", str(ROOT / "sbom_gate.py"), str(p), "--json"]
    if reviews is not None:
        r = tmp_path / "rev.json"
        r.write_text(json.dumps({"reviews": reviews}))
        cmd += ["--reviews", str(r)]
    out = subprocess.run(cmd, capture_output=True, text=True)
    return out.returncode, json.loads(out.stdout)


def test_gate_passes_clean_and_ignores_base_os(tmp_path):
    code, rep = run(tmp_path, [comp("a", "MIT"), comp("bash", "GPL-3.0-or-later", scope="base-os")])
    assert code == 0 and rep["verdict"] == "pass"


def test_lgpl_needs_a_written_review(tmp_path):
    comps = [comp("libquadmath (GCC)", "LGPL-2.1-or-later", scope="native-in-wheel")]
    code, rep = run(tmp_path, comps)
    assert code == 1 and rep["review"][0]["component"].startswith("libquadmath")
    code, rep = run(tmp_path, comps, reviews={"libquadmath (GCC)": "supply-chain/reviews/x.md"})
    assert code == 0 and rep["reviewed"]


def test_hard_fail_cannot_be_reviewed_away(tmp_path):
    code, rep = run(tmp_path, [comp("evil", "AGPL-3.0-only")], reviews={"evil": "x"})
    assert code == 1 and rep["fail"]


def test_opencv_or_ffmpeg_libs_fail_even_with_a_good_license(tmp_path):
    code, rep = run(tmp_path, [comp("opencv-python-headless", "Apache-2.0")])  # no attestation
    assert code == 1 and rep["fail"][0]["scope"] == "ffmpeg"
    yes = [{"name": "oo:opencv_ffmpeg", "value": "YES"}]
    code, rep = run(tmp_path, [comp("opencv-python-headless", "Apache-2.0", extra=yes)])
    assert code == 1 and rep["fail"][0]["scope"] == "ffmpeg"
    code, rep = run(tmp_path, [comp("FFmpeg", "LGPL-2.1-or-later", scope="native-in-wheel", files="libavcodec.so.60")], reviews={"FFmpeg": "x"})
    assert code == 1


@pytest.mark.parametrize("state", ["NO", "absent"])
def test_opencv_built_without_ffmpeg_passes(tmp_path, state):
    attest = [{"name": "oo:opencv_ffmpeg", "value": state}]
    code, rep = run(tmp_path, [comp("opencv-python-headless", "Apache-2.0", extra=attest)])
    assert code == 0 and rep["verdict"] == "pass"


def test_missing_license_fails_closed(tmp_path):
    code, rep = run(tmp_path, [comp("mystery", None)])
    assert code == 1 and rep["unknown"]
