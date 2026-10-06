# trial-report-adapter

Standalone workspace. It reads one `oo.benchmark.trial-report-adapter/v1` document from stdin and writes one `oo.benchmark.trial-statistics/v1` document to stdout. Exit 0 writes that statistics input. Exit 2 is config. The process does not compute pass^k or a bootstrap.

The rules are `CONTRACT.md`. Two trusted SQL runner reports must share ordered case ids, full trial vectors, and the version tuple. `pass` stays `pass`. Timeout, refusal, fail, and `not_run` become `fail`. `needs_reannotation` stays in the vector. `tuple.model` `none` is emitted as the comparison class `trusted_sql`.

This crate is not a member of the root workspace. The proof build points `oo-core` at the fixed archive and keeps serde 1.0.229 and serde_json 1.0.151. It does not score a question bank, certify a snapshot, or accept a benchmark release.
