"""Scene cuts and key frames from a 1 fps frame sequence (D75).

PySceneDetect 0.7.3 `ContentDetector` and `HashDetector` run on numpy frames (`scenedetect_cuts`); its video and
image-sequence readers need OpenCV videoio or PyAV, which the FFmpeg-free image does not have, so frames come
from our FFmpeg and the detectors are fed directly. Without the package, the same two signals come from numpy: a content difference (mean absolute HSV-like
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


def scenedetect_cuts(frames, hash_threshold=0.1, content_threshold=27.0):
    """Frame indexes where PySceneDetect 0.7.3 reports a cut, or `None` when it is not installed.

    The detectors run on numpy frames (`process_frame`), so no video reader and no FFmpeg are involved. Measured
    on slide recordings (S10b day one): `ContentDetector` (HSV change, default 27) misses page turns of
    sparse slides at any threshold we tried (3 to 27); `HashDetector` at 0.1 (default 0.395) finds every one.
    Both run; a cut from either counts. Frames are BGR arrays like OpenCV's.
    """
    try:
        import cv2

        if not hasattr(cv2, "VideoCapture"):
            # This OpenCV has no video reader on purpose (built without FFmpeg, see the Dockerfile); scenedetect's
            # backends module names the type at import time but the detectors below never open a video.
            cv2.VideoCapture = type("VideoCapture", (), {})
        from scenedetect import ContentDetector, FrameTimecode, HashDetector
    except ImportError:
        return None
    dets = [HashDetector(threshold=hash_threshold, min_scene_len=1), ContentDetector(threshold=content_threshold, min_scene_len=1)]
    cuts = set()
    for i, (_, img) in enumerate(frames):
        arr = np.asarray(img.convert("RGB"))[:, :, ::-1].copy()
        tc = FrameTimecode(timecode=i, fps=1.0)
        for d in dets:
            cuts.update(c.frame_num for c in d.process_frame(tc, arr))
    for d in dets:
        cuts.update(c.frame_num for c in d.post_process(FrameTimecode(timecode=len(frames), fps=1.0)))
    return {c for c in cuts if 0 < c < len(frames)}


def dedupe(frames, hash_distance=HASH_DISTANCE, use_scenedetect=True):
    """`frames`: [(t_ms, PIL image)] in time order. Returns the kept `[(t_ms, image, scene_index)]`.

    A frame is kept when its hash is more than `hash_distance` bits from the last kept frame, or when the
    content jumps by more than CUT_THRESHOLD. `scene_index` counts the kept frames (a scene starts at each).
    """
    kept, last_hash, prev_img = [], None, None
    cuts = scenedetect_cuts(frames) if use_scenedetect else None
    for i, (t, img) in enumerate(frames):
        h = dhash_bits(img)
        new = last_hash is None or hamming(h, last_hash) > hash_distance or (
            prev_img is not None and luma_change(prev_img, img) > CUT_THRESHOLD) or (cuts is not None and i in cuts)
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
