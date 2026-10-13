#!/usr/bin/env python3
"""Parser-only negative/codec checks. No source, target, sealer, or model is represented."""
import copy
import importlib.util
from pathlib import Path
spec = importlib.util.spec_from_file_location("producer", Path(__file__).with_name("native-producer.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
raw = {"protocol": "oo.mssql-preview.raw-audit/v1", "tds_query_executed": True,
       "reader": "unit_reader", "application_name": "unit_app", "events": [
           {"reader": "unit_reader", "application_name": "unit_app", "action_id": "SL", "succeeded": True}]}
assert module.classify_audit(raw, "unit_reader", "unit_app")["successful_selects"] == 1
for change in ([], [{"reader": "unit_reader", "application_name": "unit_app", "action_id": "UP", "succeeded": False}],
               [{"reader": "other", "application_name": "unit_app", "action_id": "SL", "succeeded": True}]):
    bad = copy.deepcopy(raw)
    bad["events"] = change
    try:
        module.classify_audit(bad, "unit_reader", "unit_app")
    except ValueError:
        pass
    else:
        raise AssertionError("empty/cross-reader/failed write audit accepted")
assert module.selected({"tables": [{"schema": "dbo", "name": "t", "native_apply_supported": True}]}, "dbo", "t")["name"] == "t"
try:
    module.selected({"tables": [{"schema": "dbo", "name": "t", "native_apply_supported": False}]}, "dbo", "t")
except ValueError:
    pass
else:
    raise AssertionError("unsupported key semantics accepted")
print("producer parser checks: audit identity/empty/failed-write/key refusal; no native source claimed")

try:
    module.reject_inline_secrets({"target": {"password": "unit-secret"}})
except ValueError:
    pass
else:
    raise AssertionError("inline target secret accepted")
