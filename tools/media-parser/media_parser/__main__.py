import dataclasses
import os

GIB = 1 << 30
# Media defaults (10 section 17.1, 17.8): one worker, 4 GiB blobs, 20 GiB scratch, long deadlines.
os.environ.setdefault("OO_PARSER_META", "media_parser.meta")
os.environ.setdefault("OO_PARSER_BACKEND", "media_parser.adapter")
os.environ.setdefault("OO_PARSER_WORKERS", "1")
os.environ.setdefault("OO_PARSER_PORT", "7483")
os.environ.setdefault("OO_PARSER_MAX_BLOB_BYTES", str(4 * GIB))
os.environ.setdefault("OO_PARSER_SCRATCH_BYTES", str(20 * GIB))
os.environ.setdefault("OO_PARSER_MAX_DEADLINE_MS", str(8 * 3600 * 1000))
os.environ.setdefault("OO_PARSER_MAX_RESPONSE_BYTES", str(256 << 20))

from doc_parser.config import Config  # noqa: E402

from .server import serve  # noqa: E402

if __name__ == "__main__":
    serve(dataclasses.replace(Config.from_env(), preload=("media_parser.adapter",)))
