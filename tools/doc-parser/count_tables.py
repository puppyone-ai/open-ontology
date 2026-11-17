"""Count tables and cells the sidecar adapter finds in PDFs (B-S3-01b evidence).

    docker run --rm --network none --entrypoint python -v DIR:/pdfs:ro -v $PWD/tools/doc-parser/count_tables.py:/count.py:ro IMAGE /count.py
"""
import sys, json, glob, os
sys.path.insert(0, "/opt/app")
from doc_parser import adapter
tot = 0
for p in sorted(glob.glob("/pdfs/*.pdf")):
    try:
        out = json.loads(adapter.parse(p, "pdf", None, True, 1 << 30))
    except Exception as e:
        print(os.path.basename(p), "ERR", type(e).__name__, e); continue
    d = out["doc"]
    tabs = d.get("tables", [])
    cells = sum(len(t.get("data", {}).get("table_cells", [])) for t in tabs)
    print(os.path.basename(p), "pages", out["page_count"], "tables", len(tabs), "cells", cells, "texts", len(d.get("texts", [])), out["warnings"])
    tot += len(tabs)
print("TOTAL_TABLES", tot)
