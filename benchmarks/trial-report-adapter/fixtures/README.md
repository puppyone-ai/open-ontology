# Fixtures

Handwritten pairing documents and the statistics input this adapter must write. The `.stdout` files are adapter output, not a bootstrap report.

`k2-mixed` has k of 2, four cases, and keeps `needs_reannotation`. Timeout, refusal, mismatch, and `not_run` become `fail`. `allfail` maps every trial to `fail`. `missing-trial` is a short right vector and must be refused. `tuple-mismatch` changes `project` and must be refused. `duplicate-key` is broken JSON.

`snapshot` in these files is the declared label `declared-label`. It is not a snapshot certificate.
