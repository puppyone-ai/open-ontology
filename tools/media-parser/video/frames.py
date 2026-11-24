"""Run the video steps over decoded frames (10 section 17.5).

`frames`: [(t_ms, PIL image)] at 1 fps with presentation times on the file's time axis. OCR is RapidOCR
(`doc_parser.pictures.read_lines`, PP-OCRv5 ONNX) on the key frames only.
"""
from . import keyframes as kf
from . import timeline as tl


def analyse(frames, duration_ms, read_lines, utterances=(), offset_ms=0, per_hour=kf.MAX_KEYFRAMES_PER_HOUR):
    kept = kf.dedupe(frames)
    spans = kf.scenes(kept, duration_ms)
    pick, warn = kf.cap_keyframes(spans, duration_ms, per_hour)
    lines = {i: read_lines(kept[i][1]) for i in pick}
    items = tl.build_timeline(kept, spans, lines, list(utterances), offset_ms)
    return {"key_frames": len(kept), "ocr_frames": len(pick), "scenes": len(spans), "items": items,
            "warnings": [warn] if warn else []}
