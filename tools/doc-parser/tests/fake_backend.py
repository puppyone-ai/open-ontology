"""Backend stand-in: the first bytes of the blob choose the behaviour."""
import json
import os
import signal
import time
from pathlib import Path

from doc_parser.supervisor import WorkerError


def _mode(path):
    return Path(path).read_bytes()[:8]


def _act(path):
    head = _mode(path)
    if head.startswith(b"CRASH"):
        os.kill(os.getpid(), signal.SIGSEGV)
    if head.startswith(b"KILL9"):
        events = os.environ.get("OO_PARSER_CGROUP_EVENTS")
        if events and head.startswith(b"KILL9OOM"):
            Path(events).write_text("low 0\noom 1\noom_kill 1\n")
        os.kill(os.getpid(), signal.SIGKILL)
    if head.startswith(b"SLEEP"):
        time.sleep(60)
    if head.startswith(b"ENC"):
        raise WorkerError("PARSER_ENCRYPTED", "password required")
    if head.startswith(b"BAD"):
        raise ValueError("boom")
    if head.startswith(b"BIG"):
        raise WorkerError("PARSER_OUTPUT_LIMIT", "too big")


def probe(path, fmt):
    _act(path)
    return json.dumps({"page_count": 3, "encrypted": False, "needs_password": False, "text_layer_pages": 3}).encode()


def parse(path, fmt, page_range, table, max_response, ocr=False):
    _act(path)
    return json.dumps(
        {"protocol": "oo.doc-parser/v1", "parser": {"id": "docling", "version": "fake", "adapter_version": "1"},
         "page_count": 3, "pages": page_range or [1, 3], "doc": {"name": "x", "table": table}, "ocr": ocr, "warnings": []}
    ).encode()
