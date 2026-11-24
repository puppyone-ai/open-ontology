#!/usr/bin/env python3
"""Speaker count and diarization error rate of the media-parser output against a reference timeline (lane AL).

    python3 der.py REF_DIR OUT_DIR

DER is computed on 10 ms frames at the resolution of the utterances the engine sees (an utterance is one speaker for its
whole span), with the best one-to-one mapping of labels: (missed speech + false alarm + speaker confusion) / reference speech.
"""
import itertools
import json
import sys
from pathlib import Path

FRAME = 10


def frames(segments, total):
    out = [None] * total
    for s in segments:
        for f in range(s["start_ms"] // FRAME, min(total, s["end_ms"] // FRAME)):
            out[f] = s["speaker"]
    return out


def der(ref, hyp, total):
    r, h = frames(ref, total), frames(hyp, total)
    rl = sorted({x for x in r if x}); hl = sorted({x for x in h if x})
    best = None
    pool = hl + [None] * len(rl)
    for perm in set(itertools.permutations(pool, len(rl))):
        mapping = dict(zip(rl, perm))
        err = 0
        for a, b in zip(r, h):
            if a and not b: err += 1
            elif b and not a: err += 1
            elif a and b and mapping.get(a) != b: err += 1
        best = err if best is None else min(best, err)
    speech = sum(1 for a in r if a)
    return best / max(speech, 1)


def main(ref_dir, out_dir):
    rows = []
    for ref_file in sorted(Path(ref_dir).glob("*.ref.json")):
        name = ref_file.name[:-len(".ref.json")]
        ref = json.loads(ref_file.read_text())
        out = json.loads((Path(out_dir) / f"{name}.json").read_text())
        hyp = [{"speaker": i["speaker"], "start_ms": i["t_start_ms"], "end_ms": i["t_end_ms"]} for i in out["items"] if i["kind"] == "utterance"]
        total = out["duration_ms"] // FRAME + 1
        rows.append((name, ref["speakers"], len(out["speakers"]), der(ref["segments"], hyp, total)))
    ok = sum(1 for _, r, h, _ in rows if r == h)
    for name, r, h, d in rows:
        print(f"{name:7} reference {r} speakers, found {h}, DER {d:.1%}")
    print(f"speaker count exact: {ok}/{len(rows)}; mean DER {sum(d for *_, d in rows) / len(rows):.1%}")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
