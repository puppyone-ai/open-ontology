"""Paraformer-zh (FunASR, Apache-2.0 weights, quantized ONNX export) with onnxruntime and our own front end.

The front end and decoding follow FunASR's `funasr_onnx` (MIT, (c) FunASR contributors): 80-bin Kaldi fbank (hamming, 25/10 ms,
no dither) -> low frame rate stacking (7, 6) -> CMVN from `am.mvn` -> model -> argmax over the 8404 tokens, blank 0 and eos 2
dropped. `funasr_onnx` itself is not used because its package imports librosa (numba, soundfile with libsndfile: LGPL).

Timestamps: this export has no timestamp output. Tokens are spread evenly over the *voiced* time of the chunk (short-time
energy), which keeps utterance bounds right to a fraction of a second and word bounds approximate. A model with a
timestamp predictor or the Qwen3 forced aligner would replace `place_tokens` (bake-off report).
Hot words: the plain Paraformer ignores them (the contextual model takes them); the response says so.
"""
import json
import re
from pathlib import Path

import numpy as np

SR = 16000
LFR_M, LFR_N = 7, 6
MAX_PIECE_MS = 12_000


def load_cmvn(path):
    lines = [l for l in Path(path).read_text(encoding="utf-8").splitlines() if l.split()]
    means = rescale = None
    for i, line in enumerate(lines):
        item = line.split()
        nxt = lines[i + 1].split() if i + 1 < len(lines) else []
        if item[0] == "<AddShift>" and nxt and nxt[0] == "<LearnRateCoef>":
            means = np.array(nxt[3:-1], dtype=np.float64)
        elif item[0] == "<Rescale>" and nxt and nxt[0] == "<LearnRateCoef>":
            rescale = np.array(nxt[3:-1], dtype=np.float64)
    if means is None or rescale is None:
        raise ValueError("no AddShift/Rescale in am.mvn")
    return means, rescale


def fbank(x):
    import kaldi_native_fbank as knf

    o = knf.FbankOptions()
    o.frame_opts.samp_freq = SR
    o.frame_opts.dither = 0.0
    o.frame_opts.window_type = "hamming"
    o.frame_opts.frame_shift_ms = 10.0
    o.frame_opts.frame_length_ms = 25.0
    o.mel_opts.num_bins = 80
    o.energy_floor = 0
    o.frame_opts.snip_edges = True
    fb = knf.OnlineFbank(o)
    fb.accept_waveform(SR, (np.asarray(x, dtype=np.float32) * 32768.0).tolist())
    fb.input_finished()
    n = fb.num_frames_ready
    return np.stack([fb.get_frame(i) for i in range(n)]).astype(np.float32) if n else np.zeros((0, 80), np.float32)


def lfr(feat, m=LFR_M, n=LFR_N):
    t = feat.shape[0]
    t_lfr = int(np.ceil(t / n))
    padded = np.vstack((np.tile(feat[0], ((m - 1) // 2, 1)), feat))
    t = t + (m - 1) // 2
    out = []
    for i in range(t_lfr):
        if m <= t - i * n:
            out.append(padded[i * n:i * n + m].reshape(1, -1))
        else:
            frame = padded[i * n:].reshape(-1)
            for _ in range(m - (t - i * n)):
                frame = np.hstack((frame, padded[-1]))
            out.append(frame)
    return np.vstack(out).astype(np.float32)


def merge_pieces(tokens):
    """Join `ab@@ cd` word pieces; Chinese characters stay one token each."""
    out, cur = [], ""
    for t in tokens:
        if t.endswith("@@"):
            cur += t[:-2]
        else:
            out.append(cur + t)
            cur = ""
    if cur:
        out.append(cur)
    return out


def voiced_axis(x, frame_ms=10, rel=0.03):
    """Cumulative voiced time: returns (energy mask per frame, frame_ms)."""
    f = int(SR * frame_ms / 1000)
    n = len(x) // f
    rms = np.sqrt(np.mean(np.reshape(x[:n * f], (n, f)) ** 2, axis=1)) if n else np.zeros(0)
    thr = max(1e-4, float(rms.max()) * rel) if n else 1.0
    return rms > thr, frame_ms


def place_tokens(n_tokens, x):
    """`[(start_ms, end_ms)]` per token, even over the voiced time of `x`."""
    if n_tokens == 0:
        return []
    mask, fm = voiced_axis(x)
    idx = np.flatnonzero(mask)
    if len(idx) == 0:
        total = len(x) * 1000 // SR
        return [(int(total * i / n_tokens), int(total * (i + 1) / n_tokens)) for i in range(n_tokens)]
    pos = idx.astype(np.int64) * fm
    out = []
    for i in range(n_tokens):
        a = int(i * len(idx) / n_tokens)
        b = max(a + 1, int((i + 1) * len(idx) / n_tokens))
        out.append((int(pos[a]), int(pos[min(b, len(idx)) - 1] + fm)))
    return out


class ParaformerOnnx:
    revision = "paraformer-zh-onnx-quant/vocab8404"

    def __init__(self, model_dir, threads=1):
        import onnxruntime as ort

        d = Path(model_dir)
        self.tokens = json.loads((d / "tokens.json").read_text(encoding="utf-8"))
        self.means, self.rescale = load_cmvn(d / "am.mvn")
        cfg = (d / "config.yaml").read_text(encoding="utf-8")
        m = re.search(r"predictor_bias:\s*(\d+)", cfg)
        self.pred_bias = int(m.group(1)) if m else 0
        so = ort.SessionOptions()
        so.intra_op_num_threads = threads
        so.inter_op_num_threads = 1
        self.sess = ort.InferenceSession(str(d / "model_quant.onnx"), so, providers=["CPUExecutionProvider"])
        self.params = {"model": "paraformer-zh", "export": "onnx-quant", "greedy": True, "threads": threads, "lfr": [LFR_M, LFR_N], "piece_ms": MAX_PIECE_MS}

    def tokens_of(self, x):
        feat = fbank(x)
        if len(feat) == 0:
            return []
        feat = (lfr(feat) + self.means) * self.rescale
        feat = feat[None].astype(np.float32)
        am, ntok = self.sess.run(None, {"speech": feat, "speech_lengths": np.array([feat.shape[1]], dtype=np.int32)})[:2]
        ids = am[0].argmax(axis=-1).tolist()
        ids = [i for i in ids if i not in (0, 2)]
        toks = [self.tokens[i] for i in ids][: int(ntok[0]) - self.pred_bias]
        return toks

    def pieces(self, x):
        """Sub-chunks of at most 12 s cut at silences: the quantized export loses accuracy on long inputs
        (bake-off: CER 0.35 at 25 s, 0.22 at 12 s). `(start_ms, end_ms)` inside the chunk."""
        from . import chunks as ck

        f = 512
        n = len(x) // f
        rms = np.sqrt(np.mean(np.reshape(x[:n * f], (n, f)) ** 2, axis=1)) if n else np.zeros(0)
        probs = [1.0 if r > max(1e-4, float(rms.max()) * 0.03) else 0.0 for r in rms] if n else []
        total = len(x) * 1000 // SR
        segs = ck.speech_segments(probs, total_ms=total, min_silence_ms=150, min_speech_ms=100, pad_ms=100)
        return ck.make_chunks(segs, probs, max_ms=MAX_PIECE_MS) or ([(0, total)] if total else [])

    def transcribe(self, x, hotwords=()):
        x = np.asarray(x, dtype=np.float32)
        words = []
        for a, b in self.pieces(x):
            seg = x[a * SR // 1000:b * SR // 1000]
            toks = merge_pieces(self.tokens_of(seg))
            for t, (s0, s1) in zip(toks, place_tokens(len(toks), seg)):
                words.append({"text": t, "start_ms": a + s0, "end_ms": a + s1, "conf": 0.9})
        return words
