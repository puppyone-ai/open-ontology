"""PP-StructureV3 backend, run inside a per-request child process (doc_parser.supervisor).

Offline and CPU only. The pipeline is built from the four admitted models (90 section 2.1) by directory:
PP-DocLayout_plus-L, PP-OCRv5_server_det and rec. Orientation, unwarping, formula, chart, seal and
region models are off, so nothing else is loaded and nothing is downloaded. Table structure
(SLANeXt_wired) is not run in this version: table regions come back as OCR text with the
DOCS_PARSE_TABLE_STRUCTURE_OFF warning.

Response (after `oo.doc-parser/v1` envelope): `page_list[]` with pixel boxes and the page size in points,
so the engine converts pixels to 1/100 pt with `pt / px` (D76).
"""
import json
import os
import tempfile
from pathlib import Path

from doc_parser.supervisor import WorkerError

from . import ADAPTER_VERSION
from .isa import isa_fingerprint
from .meta import WEIGHT_SUFFIX, parser_version

os.environ.setdefault("PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK", "True")
os.environ.setdefault("PADDLE_PDX_MODEL_SOURCE", "bos")  # never reached: model dirs are explicit
os.environ.setdefault("PADDLE_PDX_CACHE_HOME", tempfile.gettempdir())
# One thread: the aarch64 Paddle wheel uses OpenBLAS, which is not safe with several threads (segfault in
# predictor.run); one thread is also what makes the output repeatable. Scale with workers, not threads.
os.environ.setdefault("OMP_NUM_THREADS", "1")
os.environ.setdefault("OPENBLAS_NUM_THREADS", "1")

MODELS_DIR = os.environ.get("OO_OCR_MODELS", "/opt/models")
MODELS_LOCK = os.environ.get("OO_PARSER_MODELS_LOCK", "/opt/app/models.lock.json")
DPI = 200
MAX_SIDE_PX = 4096
# Text detection runs on the page scaled to at most this long side. Memory grows with the pixel count (about
# 6.8 GB peak per page at 1280 and 4.2 GB at 960 on linux/arm64; 12 GB and more at the full 200 dpi size); the
# recognizer still reads the full-resolution crops.
DET_MAX_SIDE = 1280
MAX_IMAGE_PIXELS = 80_000_000
IMAGE_MAGIC = (b"\x89PNG\r\n\x1a\n", b"\xff\xd8\xff", b"RIFF", b"BM", b"II*\x00", b"MM\x00*")
LABEL_KIND = {
    "doc_title": "heading", "paragraph_title": "heading", "text": "paragraph", "content": "paragraph",
    "abstract": "paragraph", "reference": "paragraph", "reference_content": "paragraph",
    "aside_text": "paragraph", "image": "paragraph", "chart": "paragraph", "vertical_text": "paragraph", "table": "table", "figure_title": "caption",
    "table_title": "caption", "chart_title": "caption", "header": "page_header", "footer": "page_footer",
    "header_image": "page_header", "footer_image": "page_footer", "footnote": "footnote",
    "vision_footnote": "footnote", "number": "page_footer", "algorithm": "code", "formula": "paragraph",
}
SKIP_LABELS = {"seal", "formula_number"}
# Pictures and charts have no block text of their own; the OCR lines inside the region are the text.
REGION_LABELS = {"image", "chart"}


def _models():
    try:
        lock = json.loads(Path(MODELS_LOCK).read_text())
    except (OSError, ValueError):
        return []
    return [f for f in lock.get("files", []) if f["name"].endswith(WEIGHT_SUFFIX)]


_PIPELINE = None


def _pipeline():
    global _PIPELINE
    if _PIPELINE is None:
        from paddleocr import PPStructureV3

        m = f"{MODELS_DIR}/PaddlePaddle--"
        _PIPELINE = PPStructureV3(
            layout_detection_model_name="PP-DocLayout_plus-L", layout_detection_model_dir=m + "PP-DocLayout_plus-L",
            text_detection_model_name="PP-OCRv5_server_det", text_detection_model_dir=m + "PP-OCRv5_server_det",
            text_recognition_model_name="PP-OCRv5_server_rec", text_recognition_model_dir=m + "PP-OCRv5_server_rec",
            use_doc_orientation_classify=False, use_doc_unwarping=False, use_textline_orientation=False,
            use_seal_recognition=False, use_table_recognition=False, use_formula_recognition=False,
            use_chart_recognition=False, use_region_detection=False, device="cpu", cpu_threads=1, text_det_limit_side_len=DET_MAX_SIDE, text_det_limit_type="max",
        )
    return _PIPELINE


def _to_bgr(img):
    import numpy as np

    return np.array(img.convert("RGB"))[:, :, ::-1].copy()


def _cap(img):
    w, h = img.size
    scale = min(1.0, MAX_SIDE_PX / max(w, h))
    if scale < 1.0:
        img = img.resize((max(1, round(w * scale)), max(1, round(h * scale))))
    return img


def _text_layer_lines(page, px_w, px_h):
    """The text layer of a pdf page as `(text, [x0, y0, x1, y1] in pixels)` lines, top to bottom.

    Why: a pdf may carry its text but not the fonts (a CID font such as STSong-Light with no font program and an
    identity ToUnicode map is what most CJK generators emit). pdfium then draws nothing on a host without that
    font (the sidecar image has no CJK font), so the rendered page is blank and the OCR has nothing to read, while
    the text layer is complete. The lines are the same text the page would show.
    """
    try:
        tp = page.get_textpage()
    except Exception:  # noqa: BLE001 - a damaged text layer only means no supplement
        return []
    try:
        pt_w, pt_h = page.get_size()
        sx, sy = px_w / pt_w, px_h / pt_h
        lines = []
        for i in range(tp.count_rects()):
            left, bottom, right, top = tp.get_rect(i)
            text = " ".join(tp.get_text_bounded(left, bottom, right, top).split())
            if text:
                lines.append((text, [round(left * sx), round((pt_h - top) * sy),
                                     round(right * sx), round((pt_h - bottom) * sy)]))
        return sorted(lines, key=lambda t: (round(t[1][1] / 12), t[1][0]))
    finally:
        tp.close()


def _compact(text):
    return "".join(text.split())


def _supplement(blocks, lines):
    """Add the text layer lines the OCR did not return, when the OCR returned under half of the layer's text.

    A page whose OCR is healthy (a scan, or a pdf with embedded fonts) is left exactly as the OCR gave it.
    Added blocks are plain text with score 1.0 (a layer is exact, not a recognition), in reading order.
    """
    layer = sum(len(_compact(t)) for t, _ in lines)
    seen = _compact("".join(b["text"] for b in blocks))
    if layer == 0 or len(seen) * 2 >= layer:
        return blocks
    out = list(blocks)
    for text, box in lines:
        if _compact(text) not in seen:
            out.append({"label": "text", "kind": "paragraph", "text": text, "bbox_px": box, "score": 1.0})
    return sorted(out, key=lambda b: (round(b["bbox_px"][1] / 12), b["bbox_px"][0]))


def _pdf_pages(path, page_range):
    import pypdfium2 as pdfium

    try:
        doc = pdfium.PdfDocument(str(path))
    except pdfium.PdfiumError as e:
        if "password" in str(e).lower():
            raise WorkerError("PARSER_ENCRYPTED", "password required") from None
        raise WorkerError("PARSER_CORRUPT", "pdf cannot be opened") from None
    try:
        n = len(doc)
        s, e = (page_range[0], min(page_range[1], n)) if page_range else (1, n)
        for no in range(s, e + 1):
            page = doc[no - 1]
            try:
                pt_w, pt_h = page.get_size()
                img = _cap(page.render(scale=DPI / 72).to_pil())
                yield no, pt_w, pt_h, img, _text_layer_lines(page, img.size[0], img.size[1])
            finally:
                page.close()
    finally:
        doc.close()


def _open_image(path):
    from PIL import Image, ImageOps

    Image.MAX_IMAGE_PIXELS = MAX_IMAGE_PIXELS
    with open(path, "rb") as f:
        head = f.read(8)
    if not head.startswith(IMAGE_MAGIC):
        raise WorkerError("PARSER_CORRUPT", "not a supported image")
    try:
        img = Image.open(path)
        img.load()
    except Exception:  # noqa: BLE001 - decompression bombs, truncated files
        raise WorkerError("PARSER_CORRUPT", "image cannot be decoded") from None
    img = ImageOps.exif_transpose(img)
    return _cap(img)


def _line_score(ocr, box):
    """Mean recognition score of the OCR lines whose centre lies inside `box`."""
    scores = []
    x0, y0, x1, y1 = box
    for poly, score in zip(ocr.get("rec_polys", []), ocr.get("rec_scores", [])):
        xs = [p[0] for p in poly]
        ys = [p[1] for p in poly]
        cx, cy = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2
        if x0 <= cx <= x1 and y0 <= cy <= y1:
            scores.append(float(score))
    return round(sum(scores) / len(scores), 4) if scores else 0.0


def _region_text(ocr, box):
    """OCR lines whose centre is inside `box`, top to bottom, left to right, joined by newlines."""
    x0, y0, x1, y1 = box
    found = []
    for poly, text in zip(ocr.get("rec_polys", []), ocr.get("rec_texts", [])):
        xs = [p[0] for p in poly]
        ys = [p[1] for p in poly]
        cx, cy = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2
        if x0 <= cx <= x1 and y0 <= cy <= y1 and str(text).strip():
            found.append((round(cy / 12), cx, str(text).strip()))
    return "\n".join(t for _, _, t in sorted(found))


def _run(img):
    out = list(_pipeline().predict(_to_bgr(img)))
    res = out[0].json["res"]
    ocr = res.get("overall_ocr_res", {})
    blocks = []
    for b in sorted(res.get("parsing_res_list", []), key=lambda b: b.get("block_order") or 0):
        label = b.get("block_label", "text")
        text = (b.get("block_content") or "").strip()
        box = [int(v) for v in b["block_bbox"]]
        if label in REGION_LABELS and not text:
            text = _region_text(ocr, box)
        if label in SKIP_LABELS or not text:
            continue
        blocks.append({"label": label, "kind": LABEL_KIND.get(label, "paragraph"), "text": text,
                       "bbox_px": box, "score": _line_score(ocr, box)})
    return blocks


def parse(path, fmt, page_range, table_structure, max_response, ocr=True):
    pages = []
    warnings = []
    if fmt == "pdf":
        with open(path, "rb") as f:
            if not f.read(5) == b"%PDF-":
                raise WorkerError("PARSER_CORRUPT", "not a pdf")
        for no, pt_w, pt_h, img, lines in _pdf_pages(path, page_range):
            pages.append({"page": no, "px_w": img.size[0], "px_h": img.size[1], "pt_w": pt_w, "pt_h": pt_h,
                          "blocks": _supplement(_run(img), lines)})
        page_count = pages[-1]["page"] if pages else 0
    else:
        img = _open_image(path)
        pages.append({"page": 1, "px_w": img.size[0], "px_h": img.size[1], "pt_w": None, "pt_h": None,
                      "blocks": _run(img)})
        page_count = 1
    if any(b["label"] == "table" for p in pages for b in p["blocks"]):
        warnings.append("DOCS_PARSE_TABLE_STRUCTURE_OFF")
    if any(b["score"] < 0.8 for p in pages for b in p["blocks"]):
        warnings.append("DOCS_PARSE_LOW_TEXT_CONFIDENCE")
    out = {
        "protocol": "oo.doc-parser/v1",
        "parser": {"id": "paddleocr", "version": parser_version(), "adapter_version": ADAPTER_VERSION},
        "kind": fmt,
        "page_count": page_count,
        "pages": [pages[0]["page"], pages[-1]["page"]] if pages else [1, 0],
        "page_list": pages,
        "profile": {"tier": "cpu", "isa": isa_fingerprint(),
                    "models": [{"name": m["name"], "sha256": m["sha256"]} for m in _models()]},
        "warnings": warnings,
    }
    body = json.dumps(out, ensure_ascii=False, separators=(",", ":")).encode()
    if len(body) > max_response:
        raise WorkerError("PARSER_OUTPUT_LIMIT", "response is over the limit")
    return body


def probe(path, fmt):  # pragma: no cover - ocr-parser has no probe (meta.PROBE_FORMATS is empty)
    raise WorkerError("PARSER_UNSUPPORTED", "no probe")
