"""Spoken language per chunk and the right recognizer for it (D98 follow-up, lane AL).

Paraformer-zh is the default for Chinese. Speech in another language (English above all) comes out of it as noise, so each chunk is first
identified by Whisper's language head (sherpa-onnx `SpokenLanguageIdentification`, Whisper base weights, MIT): a Chinese chunk goes to
Paraformer as before, any other chunk is transcribed by Whisper itself, which also copes with Chinese and English mixed in one sentence.
Whisper gives text without word times, so its words are placed evenly over the voiced time of the chunk, the same way Paraformer's tokens are
(`paraformer_onnx.place_tokens`). Without the Whisper files nothing changes: the router is not built and Paraformer takes everything.
"""
import re
from pathlib import Path

import numpy as np

from .paraformer_onnx import SR, place_tokens

WORD = re.compile(r"[A-Za-z0-9'’\-]+|[㐀-鿿]|[^\sA-Za-z0-9'’\-㐀-鿿]+")


def words_of(text):
    return [w for w in WORD.findall(text) if w.strip()]


class Whisper:
    """Whisper base (multilingual) through sherpa-onnx: language identification and transcription."""

    def __init__(self, model_dir, threads=1):
        import sherpa_onnx as so

        d = Path(model_dir)
        enc = next(d.glob("*-encoder.int8.onnx"), None) or next(d.glob("*-encoder.onnx"))
        dec = next(d.glob("*-decoder.int8.onnx"), None) or next(d.glob("*-decoder.onnx"))
        tokens = next(d.glob("*-tokens.txt"))
        self._lid = so.SpokenLanguageIdentification(so.SpokenLanguageIdentificationConfig(
            whisper=so.SpokenLanguageIdentificationWhisperConfig(encoder=str(enc), decoder=str(dec)), num_threads=threads, provider="cpu"))
        self._asr = so.OfflineRecognizer.from_whisper(encoder=str(enc), decoder=str(dec), tokens=str(tokens), language="", task="transcribe",
                                                       num_threads=threads, decoding_method="greedy_search")
        self.name = d.name

    def language(self, x):
        s = self._lid.create_stream()
        s.accept_waveform(SR, np.asarray(x, dtype=np.float32))
        return self._lid.compute(s)

    def text(self, x):
        s = self._asr.create_stream()
        s.accept_waveform(SR, np.asarray(x, dtype=np.float32))
        self._asr.decode_stream(s)
        return s.result.text.strip()


class MultilingualAsr:
    """Same interface as `ParaformerOnnx` (`revision`, `params`, `transcribe`)."""

    def __init__(self, zh, whisper):
        self.zh, self.whisper = zh, whisper
        self.revision = f"{zh.revision}+whisper-base/lid-routed"
        self.params = {**zh.params, "whisper": whisper.name, "route": "language-per-chunk"}

    def transcribe(self, x, hotwords=()):
        x = np.asarray(x, dtype=np.float32)
        if len(x) < SR // 2 or self.whisper.language(x) in ("zh", "yue"):
            return self.zh.transcribe(x, hotwords)
        toks = words_of(self.whisper.text(x))
        return [{"text": t, "start_ms": s0, "end_ms": s1, "conf": 0.8} for t, (s0, s1) in zip(toks, place_tokens(len(toks), x))]
