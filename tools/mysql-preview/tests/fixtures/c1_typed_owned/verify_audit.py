#!/usr/bin/env python3
"""Independent evidence check. No producer/loader/key-codec imports."""
import hashlib
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
audits = sorted(root.glob("independent-audit-*/manifest.json"))
assert audits, "no complete independent audit"
for manifest in audits:
    for table, counts in json.loads(manifest.read_text()).items():
        blobs = [(manifest.parent / f"{table}-{side}.json").read_bytes()
                 for side in ("source", "target")]
        rows = [json.loads(blob) for blob in blobs]
        assert rows[0] == rows[1], (manifest.parent.name, table, "values")
        for side, blob in zip(("source", "target"), blobs):
            assert hashlib.sha256(blob).hexdigest() == counts[f"{side}_sha256"]
        assert len(rows[0]) == counts["rows"] and counts["differences"] == 0
        keys = [tuple(row[:2]) if table == "main_rows" else (row[0],)
                for row in rows[0]]
        assert len(set(keys)) == len(keys), (table, "duplicate source key")
result = json.loads((root / "live-result.json").read_text())
assert result["baseline"]["main_rows"]["rows"] == 10000
assert result["after_4000i_4000u_2000d"]["main_rows"]["rows"] == 12000
assert result["reader_write_statements"] == 0
print(json.dumps({"complete_audits": len(audits), "differences": 0,
                  "baseline_main_rows": 10000, "iud_main_rows": 12000}))
