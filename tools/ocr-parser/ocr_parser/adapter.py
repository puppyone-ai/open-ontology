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
os.environ.setdefault("OMP_NUM_THREADS", "2")

MODELS_DIR = os.environ.get("OO_OCR_MODELS", "/opt/models")
MODELS_LOCK = os.environ.get("OO_PARSER_MODELS_LOCK", "/opt/app/models.lock.json")
DPI = 200
MAX_SIDE_PX = 4096
MAX_IMAGE_PIXELS = 80_000_000
IMAGE_MAGIC = (b"\x89PNG\r\n\x1a\n", b"\xff\xd8\xff", b"RIFF", b"BM", b"II*\x00", b"MM\x00*")
LABEL_KIND = {
    "doc_title": "heading", "paragraph_title": "heading", "text": "paragraph", "content": "paragraph",
    "abstract": "paragraph", "reference": "paragraph", "reference_content": "paragraph",
    "aside_text": "paragraph", "vertical_text": "paragraph", "table": "table", "figure_title": "caption",
    "table_title": "caption", "chart_title": "caption", "header": "page_header", "footer": "page_footer",
    "header_image": "page_header", "footer_image": "page_footer", "footnote": "footnote",
    "vision_footnote": "footnote", "number": "page_footer", "algorithm": "code", "formula": "paragraph",
}
SKIP_LABELS = {"image", "chart", "seal", "formula_number"}


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
            use_chart_recognition=False, use_region_detection=False, device="cpu", cpu_threads=2,
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
                yield no, pt_w, pt_h, img
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


def _run(img):
    out = list(_pipeline().predict(_to_bgr(img)))
    res = out[0].json["res"]
    ocr = res.get("overall_ocr_res", {})
    blocks = []
    for b in sorted(res.get("parsing_res_list", []), key=lambda b: b.get("block_order") or 0):
        label = b.get("block_label", "text")
        text = (b.get("block_content") or "").strip()
        if label in SKIP_LABELS or not text:
            continue
        box = [int(v) for v in b["block_bbox"]]
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
        for no, pt_w, pt_h, img in _pdf_pages(path, page_range):
            pages.append({"page": no, "px_w": img.size[0], "px_h": img.size[1], "pt_w": pt_w, "pt_h": pt_h,
                          "blocks": _run(img)})
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
