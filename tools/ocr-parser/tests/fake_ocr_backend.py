"""Backend stand-in for the ocr-parser server tests."""
import json


def parse(path, fmt, page_range, table, max_response, ocr=False):
    return json.dumps({
        "protocol": "oo.doc-parser/v1", "parser": {"id": "paddleocr", "version": "fake", "adapter_version": "1"},
        "kind": fmt, "page_count": 1, "pages": [1, 1],
        "page_list": [{"page": 1, "px_w": 100, "px_h": 200, "pt_w": 36.0, "pt_h": 72.0, "blocks": []}],
        "profile": {"tier": "cpu", "isa": "test", "models": []}, "warnings": [],
    }).encode()
