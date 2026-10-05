# Fixtures

`desk-header.jsonl`, `peek-header.jsonl`, and `count-header.jsonl` are question bindings. They are not `query-pg` output.

`desk-gold.json`, `peek-gold.json`, and `count-gold.json` are handwritten expected results plus the comparator mode. They are not produced by this adapter. The live proof prepends the header to a real `query-pg` stdout, runs this adapter, and asks the result comparator to compare that actual with the gold.
