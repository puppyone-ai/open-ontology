#!/usr/bin/env python3
"""License gate over the image SBOM (90 §3, §4).

    sbom_gate.py SBOM.json [--reviews license-reviews.json] [--json]

Fails (exit 1) on AGPL, GPL, BUSL, SSPL, Elastic-2.0, CC-BY-NC, LicenseRef-*;
on any LGPL (or GPL with a runtime exception) without a written review record;
on an unknown/missing license in a Python or native component; on any
FFmpeg library in the image or any OpenCV distribution whose build does not
attest FFmpeg off (D65). Base-OS packages are
listed but not gated here: the base image is pinned by digest and covered by
its own review. Exit 0 only when nothing fails and nothing needs review.
"""
import json
import re
import sys
from pathlib import Path

HARD_FAIL = re.compile(r"(?<![L])\b(AGPL|GPL)[-\w.+]*|BUSL|SSPL|Elastic-2\.0|CC-BY-NC[-\w.]*|LicenseRef-\S+", re.I)
EXCEPTION = re.compile(r"WITH\s+(GCC-exception|Classpath-exception|LLVM-exception)", re.I)
LGPL = re.compile(r"\bLGPL", re.I)
MPL = re.compile(r"\bMPL", re.I)
FFMPEG_LIBS = re.compile(r"^lib(avcodec|avformat|avutil|swscale|swresample|avdevice|avfilter)")


def alternatives(expr):
    """Split an OR expression; each alternative is a list of AND parts."""
    return [[p.strip() for p in alt.split(" AND ")] for alt in re.split(r"\s+OR\s+", expr)]


def classify(expr):
    """pass | review | fail for one license expression."""
    if not expr or expr.strip().upper() == "NOASSERTION":
        return "unknown"
    verdicts = []
    for alt in alternatives(expr):
        worst = "pass"
        for part in alt:
            if HARD_FAIL.search(part) and not EXCEPTION.search(part):
                v = "fail"
            elif HARD_FAIL.search(part) or LGPL.search(part):
                v = "review"  # LGPL, or GPL with a runtime exception
            elif MPL.search(part):
                v = "review"
            else:
                v = "pass"
            worst = max(worst, v, key=["pass", "review", "fail"].index)
        verdicts.append(worst)
    # An OR lets us pick the best alternative.
    return min(verdicts, key=["pass", "review", "fail"].index)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    sbom = json.loads(Path(args[0]).read_text())
    reviews = {}
    if "--reviews" in sys.argv:
        reviews = json.loads(Path(sys.argv[sys.argv.index("--reviews") + 1]).read_text()).get("reviews", {})
    findings = {"fail": [], "review": [], "unknown": [], "reviewed": []}
    ffmpeg = []
    counts = {}
    for c in sbom["components"]:
        scope = next((p["value"] for p in c.get("properties", []) if p["name"] == "oo:scope"), "")
        counts[scope] = counts.get(scope, 0) + 1
        if scope == "base-os":
            continue
        expr = " AND ".join(l.get("expression") or l.get("license", {}).get("id", "") for l in c.get("licenses", [])) or None
        v = classify(expr)
        name = c["name"]
        files = next((p["value"].split(",") for p in c.get("properties", []) if p["name"] == "oo:files"), [])
        if any(FFMPEG_LIBS.match(f) for f in files) or re.fullmatch(r"FFmpeg", name):
            ffmpeg.append(name)
        if re.match(r"opencv", name, re.I) and scope == "python":
            # The wheel must attest, from cv2.getBuildInformation(), that FFmpeg is off or not built.
            state = next((p["value"] for p in c.get("properties", []) if p["name"] == "oo:opencv_ffmpeg"), "missing")
            if state not in ("NO", "absent"):
                ffmpeg.append(f"{name} (oo:opencv_ffmpeg={state})")
        if name == "ffmpeg-lgpl-own-build" and next((p["value"] for p in c.get("properties", []) if p["name"] == "oo:ffmpeg_decode_only_lgpl"), "") != "YES":
            findings["fail"].append({"component": name, "scope": scope, "license": "configure line is not --disable-gpl --disable-nonfree (D65)"})
            continue
        if v == "pass":
            continue
        entry = {"component": f"{name} {c.get('version', '')}".strip(), "scope": scope, "license": expr}
        if v == "review" and name in reviews:
            entry["review"] = reviews[name]
            findings["reviewed"].append(entry)
        else:
            findings[v].append(entry)
    if ffmpeg:
        findings["fail"].append({"component": ", ".join(sorted(set(ffmpeg))), "scope": "ffmpeg",
                                 "license": "OpenCV or FFmpeg libraries present (D65)"})
    ok = not (findings["fail"] or findings["review"] or findings["unknown"])
    report = {"verdict": "pass" if ok else "fail", "component_counts": counts, **findings}
    if "--json" in sys.argv:
        print(json.dumps(report, indent=1, ensure_ascii=False))
    else:
        print(f"SBOM gate: {'PASS' if ok else 'FAIL'}  components={counts}")
        for k in ("fail", "review", "unknown", "reviewed"):
            for e in findings[k]:
                print(f"  {k.upper():9} {e['component']}  [{e['scope']}]  {e['license']}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
