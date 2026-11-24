"""OCR of embedded pictures (B-S9-04, D76).

Docling gives picture items with their page, bounding box and (with `generate_picture_images`) the bitmap.
Text inside the bitmap is read with RapidOCR on onnxruntime, with the PP-OCRv5 files pinned in
models.lock.json and given by path (no download). Lines come back in bitmap pixels; the engine maps them to
page coordinates from `box` and `image_size` (normalize/docling.rs), so the geometry lives in one tested place.
"""
import os

MODELS_DIR = os.environ.get("DOCLING_ARTIFACTS_PATH", "/opt/models")
RAPID_DIR = "rapidocr-ppocrv5"
MIN_PIXELS = 24  # smaller pictures are icons and bullets
MAX_PICTURES = 400


def rapid_files(models_dir=None):
    d = os.path.join(models_dir or MODELS_DIR, RAPID_DIR)
    return {k: os.path.join(d, f) for k, f in (("det", "det.onnx"), ("rec", "rec.onnx"), ("keys", "rec_keys.txt"))}


def ocr_available(models_dir=None):
    import importlib.util

    return importlib.util.find_spec("rapidocr") is not None and all(os.path.isfile(p) for p in rapid_files(models_dir).values())


def rapid_params(models_dir=None):
    from rapidocr import EngineType, ModelType, OCRVersion

    f = rapid_files(models_dir)
    eng, ver, size = EngineType.ONNXRUNTIME, OCRVersion.PPOCRV5, ModelType.MOBILE
    return {
        "Global.use_cls": False, "Global.text_score": 0.5,
        "Det.engine_type": eng, "Det.ocr_version": ver, "Det.model_type": size, "Det.model_path": f["det"],
        "Rec.engine_type": eng, "Rec.ocr_version": ver, "Rec.model_type": size, "Rec.model_path": f["rec"],
        "Rec.rec_keys_path": f["keys"],
    }


_ENGINE = None


def engine(models_dir=None):
    global _ENGINE
    if _ENGINE is None:
        from rapidocr import RapidOCR

        _ENGINE = RapidOCR(params=rapid_params(models_dir))
    return _ENGINE


def read_lines(pil_image, eng=None):
    """OCR one bitmap: `[{"text", "score", "bbox_px": [x0, y0, x1, y1]}]` in reading order (top to bottom)."""
    import numpy as np

    eng = eng or engine()
    arr = np.asarray(pil_image.convert("RGB"))[:, :, ::-1].copy()
    res = eng(arr, use_cls=False)
    if res is None or getattr(res, "txts", None) is None:
        return []
    lines = []
    for box, text, score in zip(res.boxes, res.txts, res.scores):
        text = str(text).strip()
        if not text:
            continue
        xs = [float(p[0]) for p in box]
        ys = [float(p[1]) for p in box]
        lines.append({"text": text, "score": round(float(score), 4),
                      "bbox_px": [round(min(xs), 1), round(min(ys), 1), round(max(xs), 1), round(max(ys), 1)]})
    lines.sort(key=lambda l: (round(l["bbox_px"][1] / 12), l["bbox_px"][0]))
    return lines


def picture_records(doc, eng=None):
    """OCR every picture item of a DoclingDocument that has a bitmap and a page position.

    Returns `[{"page", "index", "box": {"l","t","r","b","origin"}, "image_size": [w, h], "lines": [...]}]`;
    `index` is the 1-based picture number on its page.
    """
    out, per_page = [], {}
    for pic in doc.pictures[:MAX_PICTURES]:
        if not pic.prov:
            continue
        prov = pic.prov[0]
        per_page[prov.page_no] = per_page.get(prov.page_no, 0) + 1
        try:
            image = pic.get_image(doc)
        except Exception:  # noqa: BLE001 - a picture we cannot decode is skipped, not fatal
            image = None
        if image is None or min(image.size) < MIN_PIXELS:
            continue
        b = prov.bbox
        out.append({
            "page": prov.page_no, "index": per_page[prov.page_no],
            "box": {"l": b.l, "t": b.t, "r": b.r, "b": b.b, "origin": str(getattr(b.coord_origin, "value", b.coord_origin))},
            "image_size": [image.size[0], image.size[1]], "lines": read_lines(image, eng),
        })
    return out
