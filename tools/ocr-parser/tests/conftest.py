import importlib.util
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOC_PARSER = ROOT.parent / "doc-parser"
for p in (ROOT, DOC_PARSER, ROOT / "tests"):
    sys.path.insert(0, str(p))

# Reuse the server fixtures of the doc-parser tests (same HTTP surface, other meta and backend).
_spec = importlib.util.spec_from_file_location("doc_parser_conftest", DOC_PARSER / "tests" / "conftest.py")
_dp = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_dp)
make_service = _dp.make_service
TOKEN = _dp.TOKEN
sha = _dp.sha
