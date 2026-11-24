"""Chunking rules (D73). Pure functions on milliseconds and per-frame speech probabilities."""

MAX_CHUNK_MS = 30_000
MAX_ALIGN_WINDOW_MS = 300_000
FRAME_MS = 32  # silero-vad at 16 kHz works on 512-sample frames


def speech_segments(probs, frame_ms=FRAME_MS, threshold=0.5, min_speech_ms=250, min_silence_ms=300, pad_ms=60,
                    total_ms=None):
    """Turn per-frame speech probabilities into `[(start_ms, end_ms)]` segments.

    Silences shorter than `min_silence_ms` do not split a segment; segments shorter than `min_speech_ms` are
    dropped; every segment is padded by `pad_ms` (clipped to the file) so word edges are not cut.
    """
    segs, start, last_speech = [], None, None
    for i, p in enumerate(probs):
        t = i * frame_ms
        if p >= threshold:
            if start is None:
                start = t
            last_speech = t + frame_ms
        elif start is not None and t - last_speech >= min_silence_ms:
            segs.append([start, last_speech])
            start = None
    if start is not None:
        segs.append([start, last_speech])
    out = []
    end_all = total_ms if total_ms is not None else len(probs) * frame_ms
    for s, e in segs:
        if e - s < min_speech_ms:
            continue
        out.append((max(0, s - pad_ms), min(end_all, e + pad_ms)))
    # Padding can make neighbours touch or overlap: chunks never overlap.
    fixed = []
    for s, e in out:
        if fixed and s < fixed[-1][1]:
            s = fixed[-1][1]
        if e > s:
            fixed.append((s, e))
    return fixed


def _quietest_cut(probs, lo_ms, hi_ms, frame_ms=FRAME_MS):
    """The frame boundary in `[lo_ms, hi_ms]` with the lowest speech probability (the latest on ties)."""
    a, b = max(0, lo_ms // frame_ms), max(1, hi_ms // frame_ms)
    best, best_p = b, 2.0
    for i in range(a, min(b, len(probs) - 1) + 1):
        if probs[i] <= best_p:
            best, best_p = i, probs[i]
    return best * frame_ms


def make_chunks(segments, probs=None, max_ms=MAX_CHUNK_MS, frame_ms=FRAME_MS, search_ms=5_000):
    """Chunks of at most `max_ms`, disjoint, in order.

    Segments are joined into a chunk only across silence and only while the chunk stays within `max_ms`.
    A single speech run longer than `max_ms` is cut where speech is weakest in its last `search_ms`
    (hard cut, the words are then given to a chunk by their midpoint).
    """
    chunks = []
    cur = None
    for s, e in segments:
        # Hard-cut a run that is too long on its own.
        while e - s > max_ms:
            cut = s + max_ms
            if probs is not None:
                cut = _quietest_cut(probs, s + max_ms - search_ms, s + max_ms, frame_ms)
                if cut <= s:
                    cut = s + max_ms
            if cur:
                chunks.append(tuple(cur))
                cur = None
            chunks.append((s, cut))
            s = cut
        if cur is None:
            cur = [s, e]
        elif e - cur[0] <= max_ms:
            cur[1] = e
        else:
            chunks.append(tuple(cur))
            cur = [s, e]
    if cur:
        chunks.append(tuple(cur))
    return chunks


def alignment_windows(chunks, max_ms=MAX_ALIGN_WINDOW_MS):
    """Groups of consecutive chunk indexes whose span is at most `max_ms` (forced-aligner windows)."""
    wins, cur = [], []
    for i, (s, e) in enumerate(chunks):
        if cur and e - chunks[cur[0]][0] > max_ms:
            wins.append(cur)
            cur = []
        cur.append(i)
    if cur:
        wins.append(cur)
    return wins


def own_words(words, start_ms, end_ms):
    """Keep the words whose midpoint lies in `[start_ms, end_ms)`: each word belongs to exactly one chunk."""
    return [w for w in words if start_ms <= (w["start_ms"] + w["end_ms"]) / 2 < end_ms]
