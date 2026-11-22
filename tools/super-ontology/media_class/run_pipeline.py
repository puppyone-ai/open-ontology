#!/usr/bin/env python3
"""Run the media-parser audio pipeline on wav files and write what the sidecar would return as `items`.

    docker run --rm --network none --label oo.lane=super-ontology-al -v IN:/in:ro -v OUT:/out -v THIS_DIR:/tool:ro \
      --entrypoint python oo-docs-media-parser:dev /tool/run_pipeline.py /in /out ['{"meeting-zh": 2}']

The optional last argument gives `num_speakers` for the files whose meeting metadata names the attendees (the same
`options.num_speakers` the engine sends); every other file is clustered as the engine does by default.

Real engines (VAD, ASR, speaker clustering). One `<name>.json` per wav: `{duration_ms, speakers, items}`.
"""
import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, "/opt/app")
from asr.pipeline import Pipeline  # noqa: E402
from media_parser import engines  # noqa: E402


def main(src, dst, hints):
    import os
    from asr import diarize as dz
    if os.environ.get("WINDOW_MS"):
        dz.WINDOW_MS, dz.HOP_MS = int(os.environ["WINDOW_MS"]), int(os.environ["HOP_MS"])
        dz.windows_for.__defaults__ = (dz.WINDOW_MS, dz.HOP_MS, dz.MIN_WINDOW_MS)
    vad, eng, emb, _ = engines.load({})
    for wav in sorted(Path(src).glob("*.wav")):
        with tempfile.TemporaryDirectory() as t:
            import time
            t0 = time.time()
            p = Pipeline(vad, eng, emb, Path(t) / "chunks", Path(t) / "emb", num_speakers=hints.get(wav.stem), media_asset=wav.stem in os.environ.get("ASSETS", "").split(","))
            res = p.run(wav, offset_ms=0)
        items = [{"kind": "utterance", "speaker": u["speaker"], "t_start_ms": u["t_start_ms"], "t_end_ms": u["t_end_ms"],
                  "text": u["text"], "word_start": u["word_start"], "word_end": u["word_end"]} for u in res["utterances"]]
        out = {"duration_ms": res["audio_ms"], "speakers": res["speakers"], "items": items, "skipped": res.get("skipped"), "seconds": round(time.time() - t0, 2)}
        (Path(dst) / (wav.stem + ".json")).write_text(json.dumps(out, ensure_ascii=False, indent=1))
        print(wav.name, res["audio_ms"], res["speakers"], len(items), res.get("skipped"), out["seconds"], flush=True)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], json.loads(sys.argv[3]) if len(sys.argv) > 3 else {})
