from types import SimpleNamespace

import pytest
from conftest import TOKEN, sha  # noqa: F401

from doc_parser import meta, pictures


class FakeImage:
    def __init__(self, w, h):
        self.size = (w, h)


class FakeBBox:
    def __init__(self, l, t, r, b):
        self.l, self.t, self.r, self.b = l, t, r, b
        self.coord_origin = SimpleNamespace(value="BOTTOMLEFT")


def pic(page, bbox, image):
    prov = SimpleNamespace(page_no=page, bbox=bbox)
    return SimpleNamespace(prov=[prov], get_image=lambda doc: image)


def test_rapid_files_and_availability(tmp_path):
    assert not pictures.ocr_available(str(tmp_path))
    d = tmp_path / pictures.RAPID_DIR
    d.mkdir()
    for f in ("det.onnx", "rec.onnx", "rec_keys.txt"):
        (d / f).write_bytes(b"x")
    files = pictures.rapid_files(str(tmp_path))
    assert files["det"].endswith("det.onnx") and files["keys"].endswith("rec_keys.txt")
    # Availability needs the rapidocr package too; the test env may not have it.
    import importlib.util

    assert pictures.ocr_available(str(tmp_path)) == (importlib.util.find_spec("rapidocr") is not None)


def test_picture_records_number_pictures_per_page_and_skip_icons(monkeypatch):
    monkeypatch.setattr(pictures, "read_lines", lambda img, eng=None: [{"text": "x", "score": 0.9, "bbox_px": [0, 0, 1, 1]}])
    doc = SimpleNamespace(pictures=[
        pic(1, FakeBBox(0, 10, 10, 0), FakeImage(100, 50)),
        pic(1, FakeBBox(0, 10, 10, 0), FakeImage(8, 8)),       # icon: skipped, still counted on its page
        pic(2, FakeBBox(5, 9, 6, 1), FakeImage(60, 40)),
        SimpleNamespace(prov=[], get_image=lambda d: FakeImage(100, 100)),  # no position: skipped
        pic(1, FakeBBox(1, 2, 3, 4), None),                    # no bitmap: skipped
    ])
    recs = pictures.picture_records(doc)
    assert [(r["page"], r["index"]) for r in recs] == [(1, 1), (2, 1)]
    assert recs[0]["image_size"] == [100, 50] and recs[0]["box"]["origin"] == "BOTTOMLEFT"
    assert recs[0]["box"] == {"l": 0, "t": 10, "r": 10, "b": 0, "origin": "BOTTOMLEFT"}


def test_read_lines_orders_top_to_bottom_and_drops_empty(monkeypatch):
    import numpy as np
    from PIL import Image

    class Res:
        boxes = [[[0, 100], [50, 100], [50, 120], [0, 120]], [[0, 0], [50, 0], [50, 20], [0, 20]], [[0, 50], [9, 50], [9, 60], [0, 60]]]
        txts = ["second", "first", "  "]
        scores = [0.9, 0.8, 0.1]

    seen = {}

    def eng(arr, use_cls=False):
        seen["shape"] = np.asarray(arr).shape
        return Res()

    lines = pictures.read_lines(Image.new("RGB", (60, 130), "white"), eng)
    assert [l["text"] for l in lines] == ["first", "second"]
    assert lines[0]["bbox_px"] == [0.0, 0.0, 50.0, 20.0] and seen["shape"] == (130, 60, 3)
    assert pictures.read_lines(Image.new("RGB", (4, 4)), lambda a, use_cls=False: None) == []


def test_ocr_option_is_refused_without_the_models(make_service, monkeypatch):
    _, c, _ = make_service()
    blob = b"%PDF-x"
    c.put(blob)
    monkeypatch.setattr(pictures, "ocr_available", lambda models_dir=None: False)
    status, body = c.post("/v1/parse", {"blob": sha(blob), "format": "pdf", "options": {"ocr": True}})
    assert status == 422 and body["error"]["code"] == "PARSER_UNSUPPORTED"
    assert c.call("GET", "/v1/info")[1]["limits"]["ocr"] is False


def test_ocr_option_is_passed_to_the_backend_when_available(make_service, monkeypatch):
    _, c, _ = make_service()
    blob = b"%PDF-y"
    c.put(blob)
    monkeypatch.setattr(pictures, "ocr_available", lambda models_dir=None: True)
    status, body = c.post("/v1/parse", {"blob": sha(blob), "format": "pdf", "options": {"ocr": True}})
    assert status == 200 and body["ocr"] is True
    assert c.call("GET", "/v1/info")[1]["limits"]["ocr"] is True
    status, body = c.post("/v1/parse", {"blob": sha(blob), "format": "pdf", "options": {"ocr": False}})
    assert status == 200 and body["ocr"] is False


def test_models_lock_pins_the_rapidocr_files():
    import json
    from pathlib import Path

    lock = json.loads((Path(__file__).resolve().parents[1] / "models.lock.json").read_text())
    src = next(s for s in lock["sources"] if s.get("kind") == "url")
    assert src["license"] == "Apache-2.0" and "v3.10.0" in src["files"][0]["url"]
    locked = {f["name"]: f["sha256"] for f in lock["files"]}
    for f in src["files"]:
        assert locked[f"rapidocr-ppocrv5/{f['local']}"] == f["sha256"] and len(f["sha256"]) == 64
    assert {f["local"] for f in src["files"]} == {"det.onnx", "rec.onnx", "rec_keys.txt"}


def test_requirements_lock_has_rapidocr_but_no_opencv_wheel_or_forbidden_ocr():
    from pathlib import Path

    text = (Path(__file__).resolve().parents[1] / "requirements.lock").read_text()
    names = {l.split("==")[0] for l in text.splitlines() if "==" in l and not l.startswith((" ", "#"))}
    assert {"rapidocr", "onnxruntime"} <= names
    assert not {n for n in names if n.startswith("opencv")}
    assert not names & {"easyocr", "nemotron-ocr", "qwen-asr", "soynlp", "ocrmac"}
