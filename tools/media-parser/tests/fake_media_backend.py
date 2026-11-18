"""Backend stand-in for the media server tests: the first bytes of the blob choose the behaviour."""
import json
import time
from pathlib import Path

from media_parser import errors


def probe(path, fmt):
    return json.dumps({"container": "wav", "duration_ms": 1000, "decode": "builtin"}).encode()


def parse_task(path, fmt, opts, max_response, job_dir, sha):
    head = Path(path).read_bytes()[:8]
    if head.startswith(b"CODEC"):
        raise errors.with_details(errors.CODEC_UNAVAILABLE, "no decoder", codecs=["h264"], how_to=errors.HOW_TO_CONFIGURE)
    if head.startswith(b"LONG"):
        raise errors.with_details(errors.MEDIA_TOO_LONG, "too long", duration_ms=99)
    if head.startswith(b"SLEEP"):
        for i in range(600):
            (Path(job_dir) / "progress.json").write_text(json.dumps({"done_ms": i * 100, "total_ms": 60000}))
            time.sleep(0.1)
    (Path(job_dir) / "progress.json").write_text(json.dumps({"done_ms": 500, "total_ms": 1000}))
    time.sleep(0.3 if head.startswith(b"SLOW") else 0)
    return json.dumps({"protocol": "oo.doc-parser/v1", "kind": fmt, "items": [], "opts": opts}).encode()
