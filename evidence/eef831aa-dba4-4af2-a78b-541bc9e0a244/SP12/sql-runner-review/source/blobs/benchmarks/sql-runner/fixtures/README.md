# Fixtures

These files are handwritten typed results and one adapter header. They are inputs to the unit tests. They were not produced by the adapter, and they are not a question bank.

`note-header.json` binds one text column named `note` at ordinal 0. `gold-full.json`, `gold-prefix.json`, and `gold-changed.json` are comparator expectations. `gold-set.json` uses `row_comparator` `set`, which the result comparator refuses. The changed gold carries a canary string so a report that copies cells fails the test.
