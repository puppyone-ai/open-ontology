import os

# Select the ocr-parser metadata and backend before the shared server reads its config.
os.environ.setdefault("OO_PARSER_META", "ocr_parser.meta")
os.environ.setdefault("OO_PARSER_BACKEND", "ocr_parser.adapter")
os.environ.setdefault("OO_PARSER_WORKERS", "1")
os.environ.setdefault("OO_PARSER_PORT", "7482")

from doc_parser.config import Config  # noqa: E402
from doc_parser.server import serve  # noqa: E402

if __name__ == "__main__":
    import dataclasses

    serve(dataclasses.replace(Config.from_env(), preload=("ocr_parser.adapter",)))
