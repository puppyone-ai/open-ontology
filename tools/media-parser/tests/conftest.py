import importlib.util
import sys
import wave
from pathlib import Path

import numpy as np
import pytest

ROOT = Path(__file__).resolve().parents[1]
DOC_PARSER = ROOT.parent / "doc-parser"
for p in (ROOT, DOC_PARSER, ROOT / "tests"):
    sys.path.insert(0, str(p))

_spec = importlib.util.spec_from_file_location("doc_parser_conftest", DOC_PARSER / "tests" / "conftest.py")
_dp = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_dp)
make_service = _dp.make_service
TOKEN = _dp.TOKEN
sha = _dp.sha

SR = 16_000
FREQ = {"A": 300.0, "B": 520.0, "C": 840.0}


def synth(turns, path, gap_ms=600, tail_ms=800):
    """`turns`: [(speaker letter, duration_ms)] as tones with silence between. Returns the truth list."""
    parts, truth, t = [], [], 0
    for who, dur in turns:
        n = dur * SR // 1000
        x = 0.4 * np.sin(2 * np.pi * FREQ[who] * np.arange(n) / SR)
        parts.append(x)
        truth.append((who, t, t + dur))
        g = np.zeros(gap_ms * SR // 1000)
        parts.append(g)
        t += dur + gap_ms
    parts.append(np.zeros(tail_ms * SR // 1000))
    pcm = (np.concatenate(parts) * 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())
    return truth


class EnergyVad:
    def probs(self, x):
        n = len(x) // 512
        return [1.0 if float(np.sqrt(np.mean(x[i * 512:(i + 1) * 512] ** 2))) > 0.05 else 0.0 for i in range(n)]


def freq_of(x):
    s = np.sign(x)
    return float(np.sum(s[1:] != s[:-1]) * SR / (2 * len(x)))


class ToneAsr:
    """One word per second of tone, text names the speaker tone; deterministic."""
    revision = "tone-asr/1"
    params = {"beam": 1}

    def __init__(self):
        self.calls = 0

    def transcribe(self, x, hotwords=()):
        """Words are the loud runs of the chunk cut into pieces of at most 1 s; the text names the tone."""
        self.calls += 1
        loud = np.abs(x) > 0.05
        words, i, n = [], 0, len(x)
        while i < n:
            if not loud[i]:
                i += 1
                continue
            j = i
            while j < n and (loud[j] or (j + 3200 < n and loud[j:j + 3200].any())):
                j += 1
            k = i
            while k < j:
                e = min(k + SR, j)
                if e - k >= SR // 5:
                    f = freq_of(x[k:e][np.abs(x[k:e]) > 0.05])
                    who = min(FREQ, key=lambda w: abs(FREQ[w] - f))
                    words.append({"text": f"{who}{len(words)}", "start_ms": k * 1000 // SR, "end_ms": e * 1000 // SR, "conf": 0.95})
                k = e
            i = j
        return words


class ToneEmbedder:
    def embed(self, x):
        f = freq_of(x)
        v = np.full(4, 0.02, dtype=np.float32)
        v[min(range(3), key=lambda i: abs(list(FREQ.values())[i] - f))] = 1.0
        return v


@pytest.fixture
def tone():
    return {"vad": EnergyVad(), "engine": ToneAsr(), "embedder": ToneEmbedder(), "synth": synth}
