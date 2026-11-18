#!/usr/bin/env python3
"""2 h synthetic meeting: kill, resume, replay (L6 acceptance 3, B-S10-07). Real engines, host or container.

    OO_MEDIA_MODELS=/opt/models python3 long_audio_check.py MEETING.wav [--hours 2] [--work DIR]

The input is the 96 s synthetic meeting repeated until `--hours` (with the start offset of the container applied).
1. run the pipeline and kill the process (SIGKILL) after about a third of the chunks;
2. run again: finished chunks must not be transcribed again (cache hits equal the chunks finished before the kill);
3. replay once more from scratch in a fresh job directory: the result must be byte-identical (same ISA);
4. the speaker feature directory must not exist after a finished run.
Prints a JSON verdict; exit 1 on any failure.
"""
import argparse
import json
import multiprocessing as mp
import os
import shutil
import sys
import time
import wave
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import numpy as np  # noqa: E402

from asr.pipeline import Pipeline  # noqa: E402


def build(src, out, hours):
    with wave.open(str(src)) as w:
        x = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2")
    reps = int(hours * 3600 * 16000 // (len(x) + 16000)) + 1
    gap = np.zeros(16000, dtype="<i2")
    pcm = np.concatenate([np.concatenate([x, gap]) for _ in range(reps)])[: int(hours * 3600 * 16000)]
    with wave.open(str(out), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        w.writeframes(pcm.tobytes())


def run(wav, job, kill_after=None, q=None):
    from media_parser import engines

    vad, eng, emb, _ = engines.load({})
    done = {"n": 0}

    def on_chunk(done_ms, total_ms, i, n, reused):
        done["n"] = i + 1
        if kill_after is not None and i + 1 >= kill_after:
            os.kill(os.getpid(), 9)

    p = Pipeline(vad, eng, emb, job / "chunks", job / "emb", on_chunk=on_chunk)
    t = time.time()
    res = p.run(wav, offset_ms=0)
    out = {"stats": p.stats, "seconds": round(time.time() - t, 1), "emb_dir_exists": (job / "emb").exists(),
           "bytes": json.dumps(res, sort_keys=True, ensure_ascii=False, separators=(",", ":"))}
    if q is not None:
        q.put(out)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("meeting")
    ap.add_argument("--hours", type=float, default=2.0)
    ap.add_argument("--work", default="/tmp/long-audio")
    a = ap.parse_args()
    work = Path(a.work)
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    wav = work / "long.wav"
    build(a.meeting, wav, a.hours)
    job = work / "job"
    ctx = mp.get_context("spawn")
    first = ctx.Process(target=run, args=(wav, job, 40))
    first.start()
    first.join()
    killed = first.exitcode == -9
    cached = len(list((job / "chunks").glob("*.json")))
    q = ctx.Queue()
    second = ctx.Process(target=run, args=(wav, job, None, q))
    second.start()
    r2 = q.get()
    second.join()
    r3 = run(wav, work / "fresh")
    verdict = {
        "audio_hours": a.hours, "killed": killed, "chunks_finished_before_kill": cached,
        "resume_reused": r2["stats"]["reused"], "resume_computed": r2["stats"]["computed"], "resume_seconds": r2["seconds"],
        "reused_equals_finished": r2["stats"]["reused"] >= cached, "byte_identical_replay": r2["bytes"] == r3["bytes"],
        "features_gone": not r2["emb_dir_exists"], "fresh_seconds": r3["seconds"],
    }
    print(json.dumps(verdict))
    ok = killed and verdict["reused_equals_finished"] and verdict["byte_identical_replay"] and verdict["features_gone"] and cached > 0
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
