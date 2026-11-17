"""Scene cuts and key frames from a 1 fps frame sequence (D75).

PySceneDetect 0.7.3 `ContentDetector` and `HashDetector` can read an image sequence; where it cannot (decided on
the first day of S10b), `imagehash` gives the same two signals: a content difference (mean absolute HSV-like
change) and a perceptual-hash distance. Both paths feed `dedupe`, which keeps one frame per static screen
(a slide, a screen share) and drops a frame whose hash is within `hash_distance` of the previous kept frame.
At most 600 key frames per hour of video (never less than 60 for a short clip) go to OCR; above that, frames are sampled in proportion to the
duration of their scene and a warning is recorded.
"""
import numpy as np

MAX_KEYFRAMES_PER_HOUR = 600
HASH_SIZE = 16          # 256-bit perceptual hash
HASH_DISTANCE = 6       # bits; slides that differ by a few words stay below, a page turn goes far above
CUT_THRESHOLD = 0.12    # mean abs luma change (0..1) that marks a hard cut


def dhash_bits(img, size=HASH_SIZE):
    """Difference hash of a PIL image: `size * size` booleans, no OpenCV needed."""
    g = img.convert("L").resize((size + 1, size))
    a = np.asarray(g, dtype=np.int16)
    return (a[:, 1:] > a[:, :-1]).flatten()


def hamming(a, b):
    return int(np.count_nonzero(a != b))


def luma_change(prev, cur, size=64):
    a = np.asarray(prev.convert("L").resize((size, size)), dtype=np.float32) / 255.0
    b = np.asarray(cur.convert("L").resize((size, size)), dtype=np.float32) / 255.0
    return float(np.mean(np.abs(a - b)))


def dedupe(frames, hash_distance=HASH_DISTANCE):
    """`frames`: [(t_ms, PIL image)] in time order. Returns the kept `[(t_ms, image, scene_index)]`.

    A frame is kept when its hash is more than `hash_distance` bits from the last kept frame, or when the
    content jumps by more than CUT_THRESHOLD. `scene_index` counts the kept frames (a scene starts at each).
    """
    kept, last_hash, prev_img = [], None, None
    for t, img in frames:
        h = dhash_bits(img)
        new = last_hash is None or hamming(h, last_hash) > hash_distance or (
            prev_img is not None and luma_change(prev_img, img) > CUT_THRESHOLD)
        if new:
            kept.append((t, img, len(kept)))
            last_hash = h
        prev_img = img
    return kept


def scenes(kept, duration_ms):
    """`[(start_ms, end_ms)]` from the kept key frames: each scene runs to the next key frame."""
    out = []
    for i, (t, _, _) in enumerate(kept):
        end = kept[i + 1][0] if i + 1 < len(kept) else duration_ms
        out.append((t, max(end, t)))
    return out


MIN_BUDGET = 60  # short clips are never sampled below this


def cap_keyframes(spans, duration_ms, per_hour=MAX_KEYFRAMES_PER_HOUR, min_budget=MIN_BUDGET):
    """Indexes of the scenes that go to OCR: all of them within budget, else a sample weighted by duration.

    Returns `(indexes, warning_or_None)`. The sample takes the longest scenes first (they carry the most
    screen time), ties by order, and the result is returned in time order.
    """
    budget = max(min_budget, -(-per_hour * duration_ms // 3_600_000)) if duration_ms else per_hour
    if len(spans) <= budget:
        return list(range(len(spans))), None
    ranked = sorted(range(len(spans)), key=lambda i: (-(spans[i][1] - spans[i][0]), i))
    return sorted(ranked[:budget]), "DOCS_PARSE_KEYFRAME_SAMPLED"
