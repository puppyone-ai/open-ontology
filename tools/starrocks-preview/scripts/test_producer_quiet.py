#!/usr/bin/env python3
"""Identical C5 reads must not count as a new capture."""
import importlib.util
import json
from pathlib import Path
import tempfile

BINDING = "dddddddd-4545-4141-8181-000000003014"
PATH = Path(__file__).resolve().parents[1] / "connection-probe" / "producer.py"
spec = importlib.util.spec_from_file_location("sr_producer", PATH)
producer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(producer)

assert producer.same_capture(["b", "a"], ["a", "b"])
assert not producer.same_capture(["a"], ["a", "b"])
assert not producer.same_capture(["a"], ["b"])

with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp) / BINDING
    (root / "2").mkdir(parents=True)
    (root / "2" / "capturer.json").write_text(json.dumps({"source_generation": 13, "tables": [{"name": "uniq_dim", "rows": ["a"]}]}))
    assert producer.baseline_rows(tmp, BINDING, "uniq_dim", 13) == ["a"]
    try:
        producer.baseline_rows(tmp, BINDING, "uniq_dim", 99)
    except RuntimeError as exc:
        assert str(exc) == "quiet baseline absent"
    else:
        raise SystemExit("missing baseline returned rows")
    (root / "4").mkdir()
    (root / "4" / "capturer.json").write_text(json.dumps({"source_generation": 13, "tables": [{"name": "uniq_dim", "rows": ["b"]}]}))
    try:
        producer.baseline_rows(tmp, BINDING, "uniq_dim", 13)
    except RuntimeError as exc:
        assert str(exc) == "quiet baseline ambiguous"
    else:
        raise SystemExit("ambiguous baseline returned rows")

print("quiet_ok")
