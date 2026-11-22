#!/usr/bin/env python3
"""Word error rate (English) and character error rate (Chinese) of the pipeline's transcripts against the scripts of gen_meetings.py.

    python3 wer.py OUT_DIR [NAME...]     # OUT_DIR holds <name>.json from run_pipeline.py
"""
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import gen_meetings as g  # noqa: E402


def norm(s, zh):
    s = re.sub(r"[^\w㐀-鿿 ]", " ", s.lower())
    return list(re.sub(r"\s+", "", s)) if zh else s.split()


def edit(a, b):
    d = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        prev, d[0] = d[0], i
        for j, y in enumerate(b, 1):
            prev, d[j] = d[j], min(d[j] + 1, d[j - 1] + 1, prev + (x != y))
    return d[-1]


def main(out, names):
    rows = []
    for n in names:
        zh = n.startswith("zh")
        ref = norm(" ".join(g.TEXT_ZH if zh else g.TEXT_EN), zh)
        hyp = norm(" ".join(i["text"] for i in json.loads((Path(out) / f"{n}.json").read_text())["items"] if i["kind"] == "utterance"), zh)
        rows.append((n, "CER" if zh else "WER", edit(ref, hyp) / len(ref)))
    for n, k, v in rows:
        print(f"{n:6} {k} {v:.1%}")
    for kind in ("WER", "CER"):
        vs = [v for _, k, v in rows if k == kind]
        if vs:
            print(f"mean {kind} {sum(vs) / len(vs):.1%} over {len(vs)}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:] or ["en2", "en2b", "en3", "en3b", "zh2", "zh2b", "zh3", "zh3b"])
