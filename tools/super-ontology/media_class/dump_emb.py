#!/usr/bin/env python3
"""Window embeddings of wav files with the reference speaker of each window, for tuning the clustering offline (lane AL).

    docker run ... -v WAVDIR:/in:ro -v OUT:/out -v THIS:/tool:ro --entrypoint python oo-docs-media-parser:dev /tool/dump_emb.py /in /out

Writes `<name>.npz` with `vecs` (n, d), `spans` (n, 2) in ms and `ref` (n,) the index of the reference speaker covering most of the window, -1 for none.
"""
import json
import sys
from pathlib import Path

sys.path.insert(0, "/opt/app")
import numpy as np  # noqa: E402
from asr import chunks as ck, diarize as dz, pipeline as pl  # noqa: E402
from media_parser import engines  # noqa: E402


def main(src, dst):
    vad, _eng, emb, _ = engines.load({})
    for wav in sorted(Path(src).glob("*.wav")):
        pcm = pl.read_wav(wav)
        total = int(len(pcm) * 1000 / 16000)
        probs = list(vad.probs(pl._f32(pcm)))
        chunk_list = ck.make_chunks(ck.speech_segments(probs, total_ms=total), probs)
        spans, vecs = [], []
        for s, e in chunk_list:
            for x, y in dz.windows_for(s, e):
                spans.append((x, y))
                vecs.append(emb.embed(pl._f32(pcm[x * 16:y * 16])))
        ref_file = Path(src) / f"{wav.stem}.ref.json"
        labels = []
        if ref_file.exists():
            segs = json.loads(ref_file.read_text())["segments"]
            names = sorted({s["speaker"] for s in segs})
            for x, y in spans:
                best, best_o = -1, 0
                for s in segs:
                    o = min(y, s["end_ms"]) - max(x, s["start_ms"])
                    if o > best_o:
                        best, best_o = names.index(s["speaker"]), o
                labels.append(best)
        np.savez(Path(dst) / f"{wav.stem}.npz", vecs=np.asarray(vecs, dtype=np.float32), spans=np.asarray(spans), ref=np.asarray(labels))
        print(wav.name, len(spans), flush=True)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
