"""Long-audio pipeline (10 section 17.3). Resumable, deterministic, no speaker features kept.

    wav (16 kHz mono)  ->  VAD  ->  chunks <= 30 s  ->  ASR per chunk (cached)  ->  window embeddings (scratch)
                       ->  one clustering over the file  ->  exclusive speaker per word  ->  utterances

Interfaces (so the engines can be chosen by the bake-off, D72, and replaced in tests):

* `vad.probs(samples_f32) -> list[float]` one speech probability per 32 ms frame;
* `engine.revision`, `engine.params` (JSON-able) and `engine.transcribe(samples_f32, hotwords) ->
  [{"text", "start_ms", "end_ms", "conf"}]` with times relative to the chunk;
* `embedder.embed(samples_f32) -> vector` for one window.
"""
import wave

import numpy as np

from . import chunks as ck
from . import diarize as dz
from .cache import ChunkCache, chunk_key

SAMPLE_RATE = 16_000
MAX_UTTERANCE_CHARS = 2000
UTTERANCE_GAP_MS = 3000
LOW_CONFIDENCE = 0.8
NO_SPEECH_MS = 3000
NO_SPEECH_RATIO_BP = 800
SENTENCE_END = "。！？；!?;\n"


def read_wav(path):
    with wave.open(str(path), "rb") as w:
        if (w.getnchannels(), w.getframerate(), w.getsampwidth()) != (1, SAMPLE_RATE, 2):
            raise ValueError("wav must be 16 kHz mono 16 bit")
        pcm = w.readframes(w.getnframes())
    return np.frombuffer(pcm, dtype="<i2")


def _f32(pcm16):
    return pcm16.astype(np.float32) / 32768.0


def join_text(words):
    """Concatenate word texts; a space only between two latin or digit runs."""
    out = ""
    for w in words:
        t = w["text"]
        if out and out[-1].isascii() and out[-1].isalnum() and t[:1].isascii() and t[:1].isalnum():
            out += " "
        out += t
    return out


def split_long(text, limit=MAX_UTTERANCE_CHARS):
    """Cut at sentence ends in the second half of the window, else at `limit`."""
    if len(text) <= limit:
        return [text]
    parts, start = [], 0
    while len(text) - start > limit:
        end = start + limit
        cut = next((i + 1 for i in range(end - 1, start + limit // 2 - 1, -1) if text[i] in SENTENCE_END), end)
        parts.append(text[start:cut])
        start = cut
    parts.append(text[start:])
    return parts


def build_utterances(words, speakers, offset_ms=0):
    """Group consecutive words of one speaker into utterances (a gap over 3 s also ends one).

    `word_start`/`word_end` are 0-based indexes of the file's word list (end inclusive), so an anchor can name
    the words; a block split for length keeps its own word range. Times move to the source axis by `offset_ms`.
    """
    utts, cur = [], None
    for i, (w, sp) in enumerate(zip(words, speakers)):
        if cur and cur["speaker"] == sp and w["start_ms"] - cur["words"][-1]["end_ms"] <= UTTERANCE_GAP_MS:
            cur["words"].append(w)
            cur["idx"].append(i)
        else:
            if cur:
                utts.append(cur)
            cur = {"speaker": sp, "words": [w], "idx": [i]}
    if cur:
        utts.append(cur)
    out = []
    for u in utts:
        text, offsets = "", []
        for w in u["words"]:
            t = w["text"]
            if text and text[-1].isascii() and text[-1].isalnum() and t[:1].isascii() and t[:1].isalnum():
                text += " "
            offsets.append(len(text))
            text += t
        pieces = split_long(text)
        cuts, acc = [], 0
        for piece in pieces:
            acc += len(piece)
            cuts.append(acc)
        spans, first, k = [], 0, 0
        for i, off in enumerate(offsets):
            while k < len(cuts) - 1 and off >= cuts[k]:
                if i > first:
                    spans.append((first, i - 1))
                    first = i
                k += 1
        spans.append((first, len(u["words"]) - 1))
        if len(spans) < len(pieces):  # a piece without a word start: merge its text into the previous one
            pieces = ["".join(pieces[:len(pieces) - len(spans) + 1])] + pieces[len(pieces) - len(spans) + 1:]
        for piece, (a, b) in zip(pieces, spans):
            ws = u["words"][a:b + 1]
            conf = sum(w.get("conf", 1.0) for w in ws) / len(ws)
            utt = {"speaker": u["speaker"], "t_start_ms": ws[0]["start_ms"] + offset_ms,
                   "t_end_ms": ws[-1]["end_ms"] + offset_ms, "text": piece,
                   "word_start": u["idx"][a], "word_end": u["idx"][b], "confidence": round(conf, 4)}
            if conf < LOW_CONFIDENCE:
                utt["low_text_confidence"] = True
            out.append(utt)
    return out


class Pipeline:
    def __init__(self, vad, engine, embedder, cache_dir, emb_dir, hotwords=(), num_speakers=None,
                 cluster_threshold=0.55, on_chunk=None, media_asset=False):
        self.vad, self.engine, self.embedder = vad, engine, embedder
        self.cache = ChunkCache(cache_dir)
        self.emb = dz.EmbeddingStore(emb_dir)
        self.hotwords = list(hotwords)
        self.num_speakers = num_speakers
        self.media_asset = media_asset
        self.cluster_threshold = cluster_threshold
        self.on_chunk = on_chunk or (lambda done_ms, total_ms, i, n, reused: None)
        self.stats = {"chunks": 0, "reused": 0, "computed": 0}

    def params(self):
        return {"engine": self.engine.params, "hotwords": self.hotwords, "sr": SAMPLE_RATE}

    def run(self, wav_path, offset_ms=0):
        pcm = read_wav(wav_path)
        total_ms = int(len(pcm) * 1000 / SAMPLE_RATE)
        probs = list(self.vad.probs(_f32(pcm)))
        segs = ck.speech_segments(probs, total_ms=total_ms)
        # D98: music, ambient sound and what the engine already knows to be a media asset are not transcribed or told apart.
        speech_ms = sum(e - s for s, e in segs)
        reason = "media_asset" if self.media_asset else (
            "no_speech" if speech_ms < NO_SPEECH_MS or speech_ms * 10_000 < NO_SPEECH_RATIO_BP * max(total_ms, 1) else None)
        if reason:
            return {"audio_ms": total_ms, "start_ms": offset_ms, "speakers": [], "utterances": [], "word_count": 0, "chunks": 0,
                    "speech_ms": speech_ms, "skipped": reason}
        chunk_list = ck.make_chunks(segs, probs)
        all_words, win_spans, win_vecs = [], [], []
        for i, (s, e) in enumerate(chunk_list):
            a, b = s * SAMPLE_RATE // 1000, e * SAMPLE_RATE // 1000
            raw = pcm[a:b]
            key = chunk_key(raw.tobytes(), s, e, self.engine.revision, self.params())
            hit = self.cache.get(key)
            was_cached = hit is not None
            if hit is None:
                words = self.engine.transcribe(_f32(raw), self.hotwords)
                words = [{"text": w["text"], "start_ms": int(w["start_ms"]) + s, "end_ms": int(w["end_ms"]) + s,
                          "conf": round(float(w.get("conf", 1.0)), 4)} for w in words]
                hit = {"words": ck.own_words(words, s, e)}
                self.cache.put(key, hit)
                self.stats["computed"] += 1
            else:
                self.stats["reused"] += 1
            if not self.emb.has(key):
                spans = dz.windows_for(s, e)
                vecs = [self.embedder.embed(_f32(pcm[x * SAMPLE_RATE // 1000: y * SAMPLE_RATE // 1000])) for x, y in spans]
                self.emb.put(key, np.asarray(vecs, dtype=np.float32).reshape(len(vecs), -1) if vecs else np.zeros((0, 1)))
            m = self.emb.get(key)
            spans = dz.windows_for(s, e)
            win_spans += spans
            win_vecs += [m[j] for j in range(len(spans))]
            all_words += hit["words"]
            self.stats["chunks"] += 1
            self.on_chunk(e, total_ms, i, len(chunk_list), was_cached)
        if win_vecs:
            stacked = np.stack(win_vecs)
            raw_labels = (dz.cluster(stacked, self.cluster_threshold, self.num_speakers) if self.num_speakers
                          else dz.cluster_auto(stacked, self.cluster_threshold))
            win_labels = dz.relabel_by_first_appearance(raw_labels, [s for s, _ in win_spans])
        else:
            win_labels = []
        speakers = dz.assign_words(all_words, win_spans, win_labels)
        utterances = build_utterances(all_words, speakers, offset_ms)
        labels = sorted({u["speaker"] for u in utterances}, key=lambda s: int(s[1:]))
        self.emb.delete()  # speaker features never outlive the file (D63, D73)
        return {"audio_ms": total_ms, "start_ms": offset_ms, "speakers": labels,
                "utterances": utterances, "word_count": len(all_words), "chunks": len(chunk_list),
                "speech_ms": speech_ms, "skipped": None}
