import hashlib
import io
from pathlib import Path

import pytest
from conftest import TOKEN, sha
from doc_parser.supervisor import WorkerError

from ocr_parser import adapter, meta

HERE = Path(__file__).resolve().parents[1]


def svc(make_service, tmp_path):
    return make_service(meta="ocr_parser.meta", backend="fake_ocr_backend",
                        models_lock=str(HERE / "models.lock.json"))


def test_info_names_the_parser_models_and_licenses(make_service, tmp_path):
    _, c, _ = svc(make_service, tmp_path)
    status, info = c.call("GET", "/v1/info")
    assert status == 200
    assert info["protocol"] == "oo.doc-parser/v1" and info["parser_id"] == "paddleocr"
    assert info["profile"] == "cpu" and info["formats"] == ["pdf", "image"]
    assert info["limits"]["ocr"] is True
    names = {m["name"] for m in info["models"]}
    assert "PaddlePaddle--PP-DocLayout_plus-L/inference.pdiparams" in names
    spdx = {x["spdx"] for x in info["licenses"]}
    assert spdx == {"Apache-2.0"}
    assert all(len(x["sha256"]) == 64 for x in info["licenses"] if "sha256" in x)


def test_models_lock_only_has_admitted_models():
    import json

    lock = json.loads((HERE / "models.lock.json").read_text())
    admitted = {"PP-DocLayout_plus-L", "PP-OCRv5_server_det", "PP-OCRv5_server_rec", "SLANeXt_wired"}
    assert {s["name"] for s in lock["sources"]} == admitted
    assert all(s["license"] == "Apache-2.0" and len(s["revision"]) == 40 for s in lock["sources"])
    assert lock["files"], "files are locked with sha256"
    assert all(len(f["sha256"]) == 64 for f in lock["files"])


def test_parse_and_format_rules(make_service, tmp_path):
    _, c, _ = svc(make_service, tmp_path)
    blob = b"%PDF-fake"
    assert c.put(blob)[0] == 201
    status, body = c.post("/v1/parse", {"blob": sha(blob), "format": "pdf", "options": {"ocr": True}})
    assert status == 200 and body["parser"]["id"] == "paddleocr" and body["page_list"][0]["px_w"] == 100
    assert c.post("/v1/parse", {"blob": sha(blob), "format": "image"})[0] == 200
    assert c.post("/v1/parse", {"blob": sha(blob), "format": "docx"})[0] == 422
    assert c.post("/v1/probe", {"blob": sha(blob), "format": "pdf"})[0] == 422
    # page_range is a pdf feature
    assert c.post("/v1/parse", {"blob": sha(blob), "format": "image", "page_range": [1, 2]})[0] == 400
    assert c.call("GET", "/v1/info", token="wrong")[0] == 401


def test_block_score_uses_lines_inside_the_box():
    ocr = {"rec_polys": [[[0, 0], [10, 0], [10, 10], [0, 10]], [[100, 100], [110, 100], [110, 110], [100, 110]]],
           "rec_scores": [0.9, 0.5]}
    assert adapter._line_score(ocr, [0, 0, 20, 20]) == 0.9
    assert adapter._line_score(ocr, [0, 0, 200, 200]) == 0.7
    assert adapter._line_score(ocr, [500, 500, 600, 600]) == 0.0


def test_image_validation(tmp_path):
    from PIL import Image

    good = tmp_path / "a.png"
    Image.new("RGB", (50, 30), "white").save(good)
    assert adapter._open_image(good).size == (50, 30)
    bad = tmp_path / "b.bin"
    bad.write_bytes(b"not an image at all")
    with pytest.raises(WorkerError) as e:
        adapter._open_image(bad)
    assert e.value.code == "PARSER_CORRUPT"
    trunc = tmp_path / "c.png"
    trunc.write_bytes(good.read_bytes()[:20])
    with pytest.raises(WorkerError):
        adapter._open_image(trunc)


def test_large_images_are_capped():
    from PIL import Image

    img = Image.new("RGB", (8000, 2000), "white")
    assert max(adapter._cap(img).size) == adapter.MAX_SIDE_PX


def test_parse_shapes_the_response_without_paddle(tmp_path, monkeypatch):
    from PIL import Image

    img = tmp_path / "x.png"
    Image.new("RGB", (80, 40), "white").save(img)
    blocks = [{"label": "table", "kind": "table", "text": "a b", "bbox_px": [0, 0, 5, 5], "score": 0.5}]
    monkeypatch.setattr(adapter, "_run", lambda im: blocks)
    out = __import__("json").loads(adapter.parse(img, "image", None, True, 1 << 20))
    assert out["parser"]["id"] == "paddleocr" and out["kind"] == "image" and out["page_count"] == 1
    assert out["page_list"][0]["pt_w"] is None and out["page_list"][0]["blocks"] == blocks
    assert set(out["warnings"]) == {"DOCS_PARSE_TABLE_STRUCTURE_OFF", "DOCS_PARSE_LOW_TEXT_CONFIDENCE"}
    assert out["profile"]["tier"] == "cpu" and ":" in out["profile"]["isa"]
    with pytest.raises(WorkerError) as e:
        adapter.parse(img, "image", None, True, 10)
    assert e.value.code == "PARSER_OUTPUT_LIMIT"


def test_opencv_build_inputs_equal_the_doc_parser_ones():
    for name in ("opencv-build-requirements.lock", "opencv-build-requirements.in"):
        a = (HERE / name).read_bytes()
        b = (HERE.parent / "doc-parser" / name).read_bytes()
        assert hashlib.sha256(a).digest() == hashlib.sha256(b).digest(), name


def test_requirements_lock_has_no_opencv_wheel_and_no_forbidden_package():
    text = (HERE / "requirements.lock").read_text()
    names = {line.split("==")[0] for line in text.splitlines() if "==" in line and not line.startswith((" ", "#"))}
    assert not {n for n in names if n.startswith("opencv")}
    assert not names & {"qwen-asr", "soynlp", "easyocr", "pymupdf", "ultralytics", "extract-msg", "docling"}
    assert {"paddleocr", "paddlepaddle", "paddlex"} <= names


def test_picture_and_chart_regions_take_the_ocr_lines_inside_them():
    ocr = {"rec_polys": [[[10, 10], [50, 10], [50, 20], [10, 20]], [[10, 40], [50, 40], [50, 50], [10, 50]],
                         [[500, 500], [510, 500], [510, 510], [500, 510]]],
           "rec_texts": ["上", "下", "外"]}
    assert adapter._region_text(ocr, [0, 0, 100, 100]) == "上\n下"
    assert adapter._region_text(ocr, [900, 900, 950, 950]) == ""
    assert "chart" in adapter.REGION_LABELS and "chart" not in adapter.SKIP_LABELS


def test_detection_is_bounded_and_threads_are_one():
    import os

    assert adapter.DET_MAX_SIDE <= 1280
    assert os.environ["OMP_NUM_THREADS"] == "1" and os.environ["OPENBLAS_NUM_THREADS"] == "1"


def _blk(text, y=0):
    return {"label": "text", "kind": "paragraph", "text": text, "bbox_px": [0, y, 50, y + 10], "score": 0.9}


def test_blank_ocr_is_filled_from_the_text_layer():
    lines = [("灯塔迁移方案", [10, 10, 90, 30]), ("目标：2026年第三季度前迁移", [10, 60, 300, 80])]
    got = adapter._supplement([], lines)
    assert [b["text"] for b in got] == ["灯塔迁移方案", "目标：2026年第三季度前迁移"]
    assert all(b["score"] == 1.0 and b["kind"] == "paragraph" for b in got)


def test_healthy_ocr_is_left_alone_and_partial_ocr_gets_only_the_missing_lines():
    lines = [("alpha beta", [0, 0, 50, 10]), ("gamma delta", [0, 40, 50, 50])]
    healthy = [_blk("alpha  beta", 0), _blk("gamma delta", 40)]
    assert adapter._supplement(healthy, lines) is healthy
    assert adapter._supplement(healthy, []) is healthy
    partial = [_blk("gamma delta", 40)]
    # OCR holds 10 of 19 characters: above half, untouched
    assert adapter._supplement(partial, lines) is partial
    short = [_blk("gam", 40)]
    got = adapter._supplement(short, lines)
    assert {b["text"] for b in got} == {"alpha beta", "gamma delta", "gam"}


def test_the_golden_proposal_pdf_has_a_readable_text_layer():
    pdfium = pytest.importorskip("pypdfium2")
    pdf = HERE.parents[1] / "testing/super-ontology/golden/docs/Proposals/灯塔迁移方案.pdf"
    if not pdf.exists():
        pytest.skip("golden set not in this checkout")
    doc = pdfium.PdfDocument(str(pdf))
    try:
        page = doc[0]
        w, h = page.get_size()
        lines = adapter._text_layer_lines(page, round(w * 200 / 72), round(h * 200 / 72))
        page.close()
    finally:
        doc.close()
    assert lines and lines[0][0] == "灯塔迁移方案"
    assert "".join(t for t, _ in lines).count("陈子墨") == 1
    assert all(0 <= b[0] < b[2] and 0 <= b[1] < b[3] for _, b in lines)
